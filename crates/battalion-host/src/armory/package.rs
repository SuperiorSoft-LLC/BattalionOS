//! The package check, in the order that keeps a bad package from touching anything real:
//! plan (platform, contract) -> download with a size cap -> sha256 -> signature (pinned key)
//! -> unzip with path, count and size limits -> manifest (id, version, contract, minOs).
//!
//! Everything here writes only under `<home>/staging/`.

use super::*;
use crate::supervisor::platform_key;
use battalion_app_api::manifest::safe_rel_path;
use battalion_app_api::{CatalogApp, Feed, FeedPlatform, Manifest, ManifestError, CONTRACT};
use minisign_verify::{PublicKey, Signature};
use std::io::Write;

impl Armory {
    /// Checks that need no download: the app's and the feed's contract, then the platform entry.
    pub(crate) fn plan(&self, app: &CatalogApp, feed: &Feed) -> Result<FeedPlatform> {
        if CONTRACT < app.contract.min || CONTRACT > app.contract.max {
            return Err(ArmoryError::ContractNotSupported { id: app.id.clone(), os: CONTRACT, min: app.contract.min, max: app.contract.max });
        }
        if feed.contract > CONTRACT {
            return Err(ArmoryError::ContractNotSupported { id: app.id.clone(), os: CONTRACT, min: feed.contract, max: feed.contract });
        }
        feed.platforms.get(&platform_key()).cloned().ok_or_else(|| ArmoryError::NoPlatform(platform_key()))
    }

    /// Download into `dest`, never more than the feed's size, and check size and sha256.
    /// A connection that dies half way is an error and leaves nothing behind.
    pub(crate) fn download(&self, plat: &FeedPlatform, dest: &Path) -> Result<()> {
        let r = self.download_inner(plat, dest);
        if r.is_err() {
            let _ = std::fs::remove_file(dest);
        }
        r
    }

    fn download_inner(&self, plat: &FeedPlatform, dest: &Path) -> Result<()> {
        let limit = self.settings.max_package_bytes;
        if plat.size > limit {
            return Err(ArmoryError::TooLarge { what: "package", limit });
        }
        if let Some(d) = dest.parent() {
            std::fs::create_dir_all(d)?;
        }
        let mut rd = self.fetcher.open(&plat.url).map_err(|e| ArmoryError::Fetch { url: plat.url.clone(), reason: e.to_string() })?;
        let mut out = std::fs::File::create(dest)?;
        let mut sha = Sha256::new();
        let (mut buf, mut got) = (vec![0u8; 64 * 1024], 0u64);
        loop {
            let n = rd.read(&mut buf).map_err(|e| ArmoryError::Fetch { url: plat.url.clone(), reason: format!("connection lost after {got} bytes: {e}") })?;
            if n == 0 {
                break;
            }
            got += n as u64;
            if got > plat.size {
                return Err(ArmoryError::SizeMismatch { expected: plat.size, got });
            }
            sha.update(&buf[..n]);
            out.write_all(&buf[..n])?;
        }
        if got != plat.size {
            return Err(ArmoryError::SizeMismatch { expected: plat.size, got });
        }
        out.sync_all()?;
        if !hex(&sha.finalize()).eq_ignore_ascii_case(&plat.sha256) {
            return Err(ArmoryError::ShaMismatch);
        }
        Ok(())
    }

    /// Minisign over the package bytes, streamed. `key` is the pinned key.
    pub(crate) fn verify_signature(&self, file: &Path, signature: &str, key: &str) -> Result<()> {
        let bad = |e: minisign_verify::Error| ArmoryError::BadSignature(e.to_string());
        let key = key.trim();
        let pk = if key.contains('\n') { PublicKey::decode(key) } else { PublicKey::from_base64(key) }.map_err(bad)?;
        let sig = Signature::decode(signature.trim()).map_err(bad)?;
        let mut v = pk.verify_stream(&sig).map_err(bad)?;
        let mut f = std::fs::File::open(file)?;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            v.update(&buf[..n]);
        }
        v.finalize().map_err(bad)
    }

    /// Unzip `zip_path` into the empty-or-new folder `dest`. Refuses a path that leaves the
    /// package, a link, an encrypted or duplicate entry, and anything over the size and count
    /// limits (measured on the bytes actually written, not on what the header claims).
    pub(crate) fn unpack(&self, zip_path: &Path, dest: &Path) -> Result<()> {
        let r = self.unpack_inner(zip_path, dest);
        if r.is_err() {
            let _ = remove_tree(dest);
        }
        r
    }

    fn unpack_inner(&self, zip_path: &Path, dest: &Path) -> Result<()> {
        let st = &self.settings;
        let file = std::fs::File::open(zip_path)?;
        let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| ArmoryError::Zip(e.to_string()))?;
        if zip.len() > st.max_entries {
            return Err(ArmoryError::ZipLimit(format!("{} entries, limit {}", zip.len(), st.max_entries)));
        }
        std::fs::create_dir_all(dest)?;
        let mut seen = std::collections::BTreeSet::new();
        let (mut declared_total, mut written_total) = (0u64, 0u64);
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| ArmoryError::Zip(e.to_string()))?;
            let name = entry.name().map(|n| n.into_owned()).map_err(|e| ArmoryError::Zip(e.to_string()))?;
            let rel = name.trim_end_matches('/');
            if !safe_rel_path(rel) || rel.split(['/', '\\']).any(|s| s.ends_with('.') || s.ends_with(' ')) {
                return Err(ArmoryError::ZipPath(name));
            }
            if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
                return Err(ArmoryError::ZipPath(format!("{name} (a link)")));
            }
            if entry.encrypted() {
                return Err(ArmoryError::Zip(format!("{name} is encrypted")));
            }
            if !seen.insert(rel.to_lowercase()) {
                return Err(ArmoryError::ZipPath(format!("{name} (twice)")));
            }
            let target = dest.join(rel);
            if entry.is_dir() {
                std::fs::create_dir_all(&target)?;
                continue;
            }
            let declared = entry.size();
            if declared > st.max_file_bytes {
                return Err(ArmoryError::ZipLimit(format!("{name} is {declared} bytes, limit {}", st.max_file_bytes)));
            }
            declared_total = declared_total.saturating_add(declared);
            if declared_total > st.max_unpacked_bytes {
                return Err(ArmoryError::ZipLimit(format!("unpacked size over {}", st.max_unpacked_bytes)));
            }
            if let Some(p) = target.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(&target)?;
            let wrote = std::io::copy(&mut (&mut entry).take(declared + 1), &mut out).map_err(|e| ArmoryError::Zip(format!("{name}: {e}")))?;
            if wrote > declared {
                return Err(ArmoryError::ZipLimit(format!("{name} is larger than its header says")));
            }
            written_total += wrote;
            if written_total > st.max_unpacked_bytes {
                return Err(ArmoryError::ZipLimit(format!("unpacked size over {}", st.max_unpacked_bytes)));
            }
            out.sync_all()?;
        }
        Ok(())
    }

    /// Read `app.json` of an unpacked package and hold it to what was asked for.
    pub(crate) fn check_manifest(&self, dir: &Path, id: &str, version: &str) -> Result<Manifest> {
        let text = std::fs::read_to_string(dir.join("app.json")).map_err(|e| ArmoryError::Manifest(format!("app.json: {e}")))?;
        let m = Manifest::parse(&text, &platform_key(), CONTRACT).map_err(|e| match e {
            ManifestError::ContractNotSupported { os, min, max } => ArmoryError::ContractNotSupported { id: id.into(), os, min, max },
            other => ArmoryError::Manifest(other.to_string()),
        })?;
        if m.id != id {
            return Err(ArmoryError::IdMismatch { expected: id.into(), got: m.id });
        }
        if m.version != version {
            return Err(ArmoryError::VersionMismatch { expected: version.into(), got: m.version });
        }
        if let Some(need) = &m.min_os {
            if battalion_app_api::Version::parse(need).is_some_and(|n| n > self.os_version) {
                let p = self.os_version.parts;
                return Err(ArmoryError::OsTooOld { need: need.clone(), have: format!("{}.{}.{}", p[0], p[1], p[2]) });
            }
        }
        if let Some(spec) = m.process_for(&platform_key()) {
            if !dir.join(&spec.exec).is_file() {
                return Err(ArmoryError::Manifest(format!("process exec '{}' is not in the package", spec.exec)));
            }
        }
        Ok(m)
    }

    /// The whole check for one package: download, verify, unpack, manifest. Returns the unpacked
    /// folder inside staging and its manifest. Cleans its staging files on failure.
    pub(crate) fn fetch_and_check(&self, app: &CatalogApp, feed: &Feed, plat: &FeedPlatform, key: &str, steps: &mut dyn FnMut(&str) -> Result<()>) -> Result<(PathBuf, Manifest)> {
        let tag = format!("{}-{}", app.id, feed.version);
        let part = self.paths.staging().join(format!("{tag}.zip.part"));
        let dir = self.paths.staging().join(format!("{tag}.dir"));
        let _ = remove_tree(&dir);
        let run = (|| {
            steps("download")?;
            self.download(plat, &part)?;
            steps("verify")?;
            self.verify_signature(&part, &plat.signature, key)?;
            steps("unpack")?;
            self.unpack(&part, &dir)?;
            let m = self.check_manifest(&dir, &app.id, &feed.version)?;
            Ok(m)
        })();
        let _ = std::fs::remove_file(&part);
        match run {
            Ok(m) => Ok((dir, m)),
            Err(e) => {
                let _ = remove_tree(&dir);
                Err(e)
            }
        }
    }
}

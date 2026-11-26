// SPDX-License-Identifier: MPL-2.0
//! BattalionOS updates itself (BOS-30). The OS is not an app: it has its own signed feed (`latest.json`, the format the Tauri
//! updater writes), one pinned key, and one action: check, download + verify, stop every app, hand the installer over.
//!
//! Order that keeps a bad update from touching anything running: fetch the feed -> download -> signature (pinned key) ->
//! only then stop the apps -> write the resume list -> the desktop starts the installer and exits. The resume list is read at
//! the next start (the new version, or the old one when the installer was cancelled) and the apps that ran are started again.

use super::*;
use minisign_verify::{PublicKey, Signature};
use std::io::Write;

/// Where the OS looks for itself. Data, read from the OS's config; the key is the one in `tauri.conf.json`
/// (`plugins.updater.pubkey`: base64 of a minisign `.pub` file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsFeed {
    pub url: String,
    pub pubkey: String,
    /// The platform entry of `latest.json`, e.g. `windows-x86_64`.
    pub platform: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OsUpdate {
    pub version: String,
    pub notes: String,
    #[serde(skip)]
    pub url: String,
    #[serde(skip)]
    pub signature: String,
    /// BOS-AND-SELFUPDATE-1: the APK's sha256 (hex) from an Android feed; empty for a signed desktop installer.
    #[serde(skip)]
    pub sha256: String,
}

/// BOS-AND-SELFUPDATE-1: where BattalionOS on Android looks for itself: `bos-android.json` (written by
/// `.github/workflows/android-release.yml`: `{version, versionCode, url, sha256, commit}`). The APK is checked against the
/// feed's sha256 here; Android itself (and the OS's install step) checks that its signing certificate is the installed one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApkFeed {
    pub url: String,
    /// Test builds only (cargo feature `test-update` of battalion-android): read a loopback `http:` feed and APK.
    pub allow_loopback_http: bool,
}

#[derive(Deserialize)]
struct ApkLatest {
    version: String,
    url: String,
    sha256: String,
}

/// An app that reports work in flight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BusyApp {
    pub id: String,
    pub name: String,
    pub busy: u32,
}

#[derive(Deserialize)]
struct LatestPlatform {
    url: String,
    signature: String,
}
#[derive(Deserialize)]
struct Latest {
    version: String,
    #[serde(default)]
    notes: String,
    platforms: std::collections::BTreeMap<String, LatestPlatform>,
}

#[derive(Serialize, Deserialize, Default)]
struct Resume {
    apps: Vec<String>,
}

const MAX_OS_FEED: u64 = 256 << 10;
const MAX_OS_PACKAGE: u64 = 1 << 30;

fn decode_text(b64: &str, what: &str) -> Result<String> {
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|e| ArmoryError::BadSignature(format!("{what} is not base64: {e}")))?;
    String::from_utf8(raw).map_err(|_| ArmoryError::BadSignature(format!("{what} is not text")))
}

impl Armory {
    /// Is a newer BattalionOS offered? `Ok(None)`: same or older. An error (no feed, 404, offline, a broken file) is the caller's to
    /// keep quiet about: the Armory works without it.
    pub fn os_check(&self, feed: &OsFeed, current: &str) -> Result<Option<OsUpdate>> {
        let bytes = self.fetch_small(&feed.url, MAX_OS_FEED, "OS feed")?;
        let latest: Latest = serde_json::from_slice(&bytes).map_err(|e| ArmoryError::Feed(format!("OS feed: {e}")))?;
        let newer = match (Version::parse(&latest.version), Version::parse(current)) {
            (Some(n), Some(o)) => n > o,
            _ => return Err(ArmoryError::Feed(format!("OS feed version '{}' or own version '{current}' is not major.minor.patch", latest.version))),
        };
        if !newer {
            return Ok(None);
        }
        let plat = latest.platforms.get(&format!("{}-msi", feed.platform)).or_else(|| latest.platforms.get(&feed.platform)).ok_or_else(|| ArmoryError::NoPlatform(feed.platform.clone()))?;
        Ok(Some(OsUpdate { version: latest.version, notes: latest.notes, url: plat.url.clone(), signature: plat.signature.clone(), sha256: String::new() }))
    }

    /// Download the installer into `<home>/staging` and check its signature against the pinned key. A bad signature, a short
    /// download or a connection that dies leaves no file and nothing else touched. `on_bytes(got, total)`: total is 0 while unknown.
    pub fn os_stage(&self, feed: &OsFeed, upd: &OsUpdate, on_bytes: &mut dyn FnMut(u64, u64)) -> Result<PathBuf> {
        let dir = self.paths.staging();
        std::fs::create_dir_all(&dir)?;
        let safe: String = upd.version.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' { c } else { '_' }).collect();
        let dest = dir.join(format!("BattalionOS-{safe}.msi"));
        let r = self.os_download(feed, upd, &dest, on_bytes);
        if r.is_err() {
            let _ = std::fs::remove_file(&dest);
        }
        r.map(|_| dest)
    }

    /// Stream `url` into `dest` (capped at `MAX_OS_PACKAGE`); returns the sha256 (hex) of what was written.
    fn os_fetch_to(&self, fetcher: &dyn Fetcher, url: &str, dest: &Path, what: &'static str, on_bytes: &mut dyn FnMut(u64, u64)) -> Result<String> {
        let mut rd = fetcher.open(url).map_err(|e| ArmoryError::Fetch { url: url.to_string(), reason: e.to_string() })?;
        let mut out = std::fs::File::create(dest)?;
        let (mut buf, mut got) = (vec![0u8; 64 * 1024], 0u64);
        let mut hash = Sha256::new();
        let mut pace = progress::Pace::new();
        loop {
            let n = rd.read(&mut buf).map_err(|e| ArmoryError::Fetch { url: url.to_string(), reason: format!("connection lost after {got} bytes: {e}") })?;
            if n == 0 {
                break;
            }
            got += n as u64;
            if got > MAX_OS_PACKAGE {
                return Err(ArmoryError::TooLarge { what, limit: MAX_OS_PACKAGE });
            }
            out.write_all(&buf[..n])?;
            hash.update(&buf[..n]);
            if pace.tick(got, 0).is_some() {
                on_bytes(got, 0);
            }
        }
        out.sync_all()?;
        on_bytes(got, got);
        Ok(hex(&hash.finalize()))
    }

    fn os_download(&self, feed: &OsFeed, upd: &OsUpdate, dest: &Path, on_bytes: &mut dyn FnMut(u64, u64)) -> Result<()> {
        self.os_fetch_to(&*self.fetcher, &upd.url, dest, "installer", on_bytes)?;
        let mut buf = vec![0u8; 64 * 1024];
        let pk_text = decode_text(&feed.pubkey, "the OS key")?;
        let sig_text = decode_text(&upd.signature, "the installer signature")?;
        let bad = |e: minisign_verify::Error| ArmoryError::BadSignature(e.to_string());
        let pk = PublicKey::decode(&pk_text).map_err(bad)?;
        let sig = Signature::decode(&sig_text).map_err(bad)?;
        let mut v = pk.verify_stream(&sig).map_err(bad)?;
        let mut f = std::fs::File::open(dest)?;
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            v.update(&buf[..n]);
        }
        v.finalize().map_err(bad)
    }

    fn apk_fetcher(&self, feed: &ApkFeed) -> Arc<dyn Fetcher> {
        if feed.allow_loopback_http {
            Arc::new(DefaultFetcher { timeout_ms: self.settings.http_timeout_ms, allow_loopback_http: true })
        } else {
            self.fetcher.clone()
        }
    }

    /// BOS-AND-SELFUPDATE-1: is a newer BattalionOS for Android offered? `Ok(None)`: same or older. An error (no feed, 404,
    /// offline, a broken file) is the caller's to keep quiet about, as with [`Armory::os_check`].
    pub fn os_check_apk(&self, feed: &ApkFeed, current: &str) -> Result<Option<OsUpdate>> {
        let rd = self.apk_fetcher(feed).open(&feed.url).map_err(|e| ArmoryError::Fetch { url: feed.url.clone(), reason: e.to_string() })?;
        let mut bytes = Vec::new();
        rd.take(MAX_OS_FEED + 1).read_to_end(&mut bytes).map_err(|e| ArmoryError::Fetch { url: feed.url.clone(), reason: e.to_string() })?;
        if bytes.len() as u64 > MAX_OS_FEED {
            return Err(ArmoryError::TooLarge { what: "OS feed", limit: MAX_OS_FEED });
        }
        let latest: ApkLatest = serde_json::from_slice(&bytes).map_err(|e| ArmoryError::Feed(format!("OS feed: {e}")))?;
        let newer = match (Version::parse(&latest.version), Version::parse(current)) {
            (Some(n), Some(o)) => n > o,
            _ => return Err(ArmoryError::Feed(format!("OS feed version '{}' or own version '{current}' is not major.minor.patch", latest.version))),
        };
        if !newer {
            return Ok(None);
        }
        let sha = latest.sha256.trim().to_ascii_lowercase();
        if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ArmoryError::Feed("OS feed: sha256 is not 64 hex digits".into()));
        }
        Ok(Some(OsUpdate { version: latest.version, notes: String::new(), url: latest.url, signature: String::new(), sha256: sha }))
    }

    /// Download the APK into `<home>/staging` and check it against the feed's sha256. A wrong hash, a short download or a
    /// connection that dies leaves no file. Nothing is stopped here: the caller stops the apps once the APK is good.
    pub fn os_stage_apk(&self, feed: &ApkFeed, upd: &OsUpdate, on_bytes: &mut dyn FnMut(u64, u64)) -> Result<PathBuf> {
        let dir = self.paths.staging();
        std::fs::create_dir_all(&dir)?;
        // One staged APK at a time: an older one left by a cancelled update goes.
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().ends_with(".apk") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        let safe: String = upd.version.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' { c } else { '_' }).collect();
        let dest = dir.join(format!("BattalionOS-{safe}.apk"));
        let r = self.os_fetch_to(&*self.apk_fetcher(feed), &upd.url, &dest, "APK", on_bytes).and_then(|got| {
            if got == upd.sha256 {
                Ok(())
            } else {
                Err(ArmoryError::BadSignature(format!("the downloaded BattalionOS {} does not match the update feed (sha256 {got}, expected {})", upd.version, upd.sha256)))
            }
        });
        if r.is_err() {
            let _ = std::fs::remove_file(&dest);
        }
        r.map(|_| dest)
    }

    /// Download + verify, and only then stop the apps. Returns the installer and the apps that were running (to start again).
    pub fn os_prepare(&self, feed: &OsFeed, upd: &OsUpdate, on_bytes: &mut dyn FnMut(u64, u64)) -> Result<(PathBuf, Vec<String>)> {
        let msi = self.os_stage(feed, upd, on_bytes)?;
        match self.os_stop_for_update() {
            Ok(apps) => Ok((msi, apps)),
            Err(e) => {
                let _ = std::fs::remove_file(&msi);
                Err(e)
            }
        }
    }

    fn resume_path(&self) -> PathBuf {
        self.paths.root().join("os-update-resume.json")
    }

    /// Ids of the apps that are running now.
    pub fn os_running_apps(&self) -> Vec<String> {
        self.installed().map(|l| l.into_iter().filter(|a| a.state == "running").map(|a| a.id).collect()).unwrap_or_default()
    }

    /// Running apps that report work in flight (the confirm names them).
    pub fn os_busy_apps(&self) -> Vec<BusyApp> {
        if !self.settings.check_busy {
            return Vec::new();
        }
        let list = self.installed().map(|l| l.into_iter().collect::<Vec<_>>()).unwrap_or_default();
        list.iter()
            .filter(|a| a.state == "running")
            .filter_map(|a| {
                let n = self.sup.health(&a.id).ok()?.busy.filter(|n| *n > 0)?;
                Some(BusyApp { id: a.id.clone(), name: a.name.clone().unwrap_or_else(|| a.id.clone()), busy: n })
            })
            .collect()
    }

    /// The point of no return: stop every running app the normal way, the resume list written first. When an app cannot be
    /// stopped, the ones already stopped are started again and the update is refused: nothing changed.
    pub fn os_stop_for_update(&self) -> Result<Vec<String>> {
        let running = self.os_running_apps();
        store::write_json(&self.resume_path(), &Resume { apps: running.clone() })?;
        let failed = self.sup.stop_all();
        if let Some((id, why)) = failed.into_iter().next() {
            self.os_resume_apps();
            return Err(ArmoryError::WrongState(format!("could not stop app '{id}': {why}")));
        }
        Ok(running)
    }

    /// Start again the apps the resume list names (after an update, or after an installer that was cancelled), then drop the
    /// list. Returns (id, error) of an app that did not start.
    pub fn os_resume_apps(&self) -> Vec<(String, String)> {
        let path = self.resume_path();
        let Ok(Some(r)) = store::read_json::<Resume>(&path) else { return Vec::new() };
        let _ = std::fs::remove_file(&path);
        r.apps.into_iter().filter_map(|id| self.sup.start(&id).err().map(|e| (id, e.to_string()))).collect()
    }

    /// Is there a resume list waiting?
    pub fn os_resume_pending(&self) -> bool {
        self.resume_path().is_file()
    }
}

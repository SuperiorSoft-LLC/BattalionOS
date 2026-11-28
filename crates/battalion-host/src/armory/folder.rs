// SPDX-License-Identifier: MPL-2.0
//! ARMORY-LOCAL-2: install an app from a folder or a package file on this machine ("Install from folder…").
//!
//! ```text
//! pick    a .bosapp file | a folder with app.json (an unpacked app) | a folder holding a .bosapp for this platform
//!         (also in its out/ or dist/: the template project's folder works, its package script writes out/) | else the
//!         project's dist-app/ (the template's built, unpacked app)
//! check   size cap -> signature when a .minisig is beside it (a bad one is refused) -> unpack (same limits)
//!         -> manifest (same checks as a catalog install) -> pages-only check where it applies
//! ask     unsigned, or signed with a key this OS does not already trust: NeedsConfirm(Unsigned) until confirmed
//!         installed from a catalog: NeedsConfirm(ReplacesCatalog) until confirmed
//! place   new app: like a catalog install (journaled, init).  Installed: a newer version goes through the
//!         update switch (stop, switch, start, rolled back when it never gets ready); the same version is replaced
//! ```
//!
//! Trusted key: this device's local key, or the key the catalog lists for that app id. A key that ships in the
//! same folder as the package proves only that the files match each other, so it counts as unsigned.
//! What was installed this way is remembered in `apps/<id>/local.json`; the Armory marks such an app "Local".

use super::*;
use crate::journal::Journal;
use crate::state::{AppState, Current};
use crate::supervisor::platform_key;
use battalion_app_api::Manifest;
use std::io::Write;

/// How a folder package is signed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Signing {
    /// Verified with this device's local key or the catalog's key for the app.
    Trusted,
    /// Verified, but with a key only the folder vouches for (a throwaway key).
    OwnKey,
    /// No signature.
    Unsigned,
}

/// What an install from a folder needs the person to agree to first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirm {
    /// The package is unsigned or signed with a key this OS does not trust.
    Unsigned { id: String, version: String, signing: Signing },
    /// The app is installed from a catalog; the local one would replace it.
    ReplacesCatalog { id: String, installed: String },
}

impl std::fmt::Display for Confirm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Confirm::Unsigned { id, version, signing: Signing::OwnKey } => write!(f, "unsigned local app: '{id}' {version} is signed with a key this OS does not know; install it only if you built it"),
            Confirm::Unsigned { id, version, .. } => write!(f, "unsigned local app: '{id}' {version} has no signature; install it only if you built it"),
            Confirm::ReplacesCatalog { id, installed } => write!(f, "'{id}' {installed} is installed from the catalog; installing the local one replaces it"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FolderOptions {
    /// The person agreed to install an unsigned (or own-key) local app.
    pub allow_unsigned: bool,
    /// The person agreed to replace an app installed from a catalog.
    pub replace_catalog: bool,
}

/// `apps/<id>/local.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalInstall {
    /// The folder or file it was installed from (picked again by "Reinstall from folder").
    pub path: String,
    pub signing: Signing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderInstalled {
    pub id: String,
    pub version: String,
    /// The version it replaced, when it was installed already.
    pub from: Option<String>,
    pub signing: Signing,
}

fn local_path(paths: &Paths, id: &str) -> Result<PathBuf> {
    Ok(paths.app_dir(id)?.join("local.json"))
}

/// The package a picked path names, and what signs it. `zipped` = made from an unpacked folder (a staging file).
struct Picked {
    package: PathBuf,
    signature: Option<String>,
    key: Option<String>,
    zipped: bool,
}

impl Armory {
    /// The `local.json` of an installed app, when it came from a folder.
    pub fn local_install(&self, id: &str) -> Option<LocalInstall> {
        store::read_json(&local_path(&self.paths, id).ok()?).ok().flatten()
    }

    /// Install (or reinstall) the app at `path`. Returns `Err(NeedsConfirm)` when the person has to agree first;
    /// call again with that option set.
    pub fn install_from_path(&self, path: &Path, opts: &FolderOptions) -> Result<FolderInstalled> {
        let picked = self.pick(path)?;
        let r = self.install_picked(path, &picked, opts);
        if picked.zipped {
            let _ = std::fs::remove_file(&picked.package);
        }
        r
    }

    fn pick(&self, path: &Path) -> Result<Picked> {
        let bad = |m: String| ArmoryError::Io(m);
        if path.is_file() {
            return Ok(self.picked_file(path, path.parent().unwrap_or(path)));
        }
        if !path.is_dir() {
            return Err(bad(format!("{} is not a folder or a package file", path.display())));
        }
        let unpacked = |dir: &Path| -> Result<Picked> {
            let zip = self.paths.staging().join(format!("local-{}.zip", std::process::id()));
            self.zip_folder(dir, &zip)?;
            Ok(Picked { package: zip, signature: None, key: None, zipped: true })
        };
        if path.join("app.json").is_file() {
            return unpacked(path);
        }
        for dir in [path.to_path_buf(), path.join("out"), path.join("dist")] {
            let plat = platform_key();
            let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "bosapp"))
                .collect();
            // A packer names one file per platform; prefer this platform's, then any single one.
            let mine: Vec<PathBuf> = found.iter().filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().contains(&plat))).cloned().collect();
            if !mine.is_empty() {
                found = mine;
            }
            found.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
            if let Some(p) = found.pop() {
                return Ok(self.picked_file(&p, &dir));
            }
        }
        if path.join("dist-app").join("app.json").is_file() {
            return unpacked(&path.join("dist-app"));
        }
        Err(bad(format!("no app found in {}: pick a folder with app.json, a folder holding a .bosapp package, or the .bosapp file", path.display())))
    }

    fn picked_file(&self, file: &Path, dir: &Path) -> Picked {
        let mut sig_path = file.as_os_str().to_owned();
        sig_path.push(".minisig");
        let signature = std::fs::read_to_string(PathBuf::from(sig_path)).ok();
        // The key a packer writes beside the package: public.key, or catalog-entry.json's publicKey.
        let key = std::fs::read_to_string(dir.join("public.key")).ok().or_else(|| {
            let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("catalog-entry.json")).ok()?).ok()?;
            v.get("publicKey")?.as_str().map(str::to_string)
        });
        Picked { package: file.to_path_buf(), signature, key, zipped: false }
    }

    /// Zip an unpacked app folder (no links, the entry and size limits apply).
    fn zip_folder(&self, dir: &Path, dest: &Path) -> Result<()> {
        std::fs::create_dir_all(self.paths.staging())?;
        let out = std::fs::File::create(dest)?;
        let mut w = zip::ZipWriter::new(out);
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let (mut count, mut total) = (0usize, 0u64);
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d)?.flatten() {
                let ft = e.file_type()?;
                let p = e.path();
                if ft.is_symlink() {
                    return Err(ArmoryError::ZipPath(format!("{} (a link)", p.display())));
                }
                if ft.is_dir() {
                    stack.push(p);
                    continue;
                }
                count += 1;
                total += e.metadata()?.len();
                if count > self.settings.max_entries || total > self.settings.max_package_bytes {
                    return Err(ArmoryError::TooLarge { what: "app folder", limit: self.settings.max_package_bytes });
                }
                let rel = p.strip_prefix(dir).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                w.start_file(rel, o).map_err(|e| ArmoryError::Zip(e.to_string()))?;
                w.write_all(&std::fs::read(&p)?)?;
            }
        }
        w.finish().map_err(|e| ArmoryError::Zip(e.to_string()))?;
        Ok(())
    }

    fn install_picked(&self, path: &Path, picked: &Picked, opts: &FolderOptions) -> Result<FolderInstalled> {
        let size = std::fs::metadata(&picked.package)?.len();
        if size > self.settings.max_package_bytes {
            return Err(ArmoryError::TooLarge { what: "package", limit: self.settings.max_package_bytes });
        }
        // A signature that is there must hold, whatever the key.
        let verified_key = match (&picked.signature, &picked.key) {
            (Some(sig), Some(key)) => {
                self.verify_signature(&picked.package, sig, key)?;
                // A whole .pub file holds a comment line; the key is its base64 line (what catalogs and pins carry).
                let line = key.lines().map(str::trim).rfind(|l| !l.is_empty() && !l.starts_with("untrusted comment")).unwrap_or(key.trim());
                Some(line.to_string())
            }
            (Some(_), None) => return Err(ArmoryError::BadSignature("the package has a signature but no public.key or catalog-entry.json beside it".into())),
            _ => None,
        };

        let dir = self.paths.staging().join(format!("local-{}.dir", std::process::id()));
        let _ = remove_tree(&dir);
        let checked = (|| -> Result<Manifest> {
            self.unpack(&picked.package, &dir)?;
            let text = std::fs::read_to_string(dir.join("app.json")).map_err(|e| ArmoryError::Manifest(format!("app.json: {e}")))?;
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| ArmoryError::Manifest(e.to_string()))?;
            let (id, version) = (v.get("id").and_then(|x| x.as_str()).unwrap_or(""), v.get("version").and_then(|x| x.as_str()).unwrap_or(""));
            self.paths.app_dir(id)?;
            let m = self.check_manifest(&dir, id, version)?;
            if self.settings.pages_only {
                package::pages_only_check(&dir, &m)?;
            }
            Ok(m)
        })();
        let manifest = match checked {
            Ok(m) => m,
            Err(e) => {
                let _ = remove_tree(&dir);
                return Err(e);
            }
        };
        let r = self.place_local(path, &dir, &manifest, verified_key, opts);
        let _ = remove_tree(&dir);
        r
    }

    fn place_local(&self, path: &Path, dir: &Path, m: &Manifest, key: Option<String>, opts: &FolderOptions) -> Result<FolderInstalled> {
        let id = m.id.as_str();
        let trusted = key.as_deref().map(str::trim).is_some_and(|k| {
            localrepo::device_key(&self.paths).as_deref().map(str::trim) == Some(k) || self.catalog().ok().and_then(|l| l.apps.into_iter().find(|e| e.app.id == id)).is_some_and(|e| e.app.public_key.trim() == k)
        });
        let signing = match (&key, trusted) {
            (Some(_), true) => Signing::Trusted,
            (Some(_), false) => Signing::OwnKey,
            (None, _) => Signing::Unsigned,
        };
        if signing != Signing::Trusted && !opts.allow_unsigned {
            return Err(ArmoryError::NeedsConfirm(Confirm::Unsigned { id: id.into(), version: m.version.clone(), signing }));
        }
        let lock = self.lock_app(id)?;
        let cur = self.load_current(id)?;
        if let Some(c) = &cur {
            if self.local_install(id).is_none() && !opts.replace_catalog {
                return Err(ArmoryError::NeedsConfirm(Confirm::ReplacesCatalog { id: id.into(), installed: c.version.clone() }));
            }
        }
        let local = LocalInstall { path: path.display().to_string(), signing };
        let r = match cur {
            None => self.local_fresh(dir, m, key.unwrap_or_default()),
            Some(c) => self.local_replace(dir, m, c, key.unwrap_or_default()),
        };
        let r = r.and_then(|from| {
            store::write_json(&local_path(&self.paths, id)?, &local)?;
            Ok(FolderInstalled { id: id.into(), version: m.version.clone(), from, signing })
        });
        drop(lock);
        if r.is_err() && matches!(self.load_current(id), Ok(None)) {
            if let Ok(d) = self.paths.app_dir(id) {
                let _ = std::fs::remove_file(d.join("armory.lock"));
                let _ = std::fs::remove_dir(d);
            }
        }
        r
    }

    /// A new app: the catalog install's place and init, journaled, undone on failure.
    fn local_fresh(&self, dir: &Path, m: &Manifest, key: String) -> Result<Option<String>> {
        let id = m.id.as_str();
        let (j, tx): (Journal, u64) = self.begin_tx(id, "install", &m.version)?;
        let r = (|| -> Result<()> {
            let cur_path = self.paths.current_json(id)?;
            let mut cur = Current { version: m.version.clone(), previous: None, state: AppState::Absent, public_key: key };
            // Downloaded and verified already (the picked file was checked before this): same states as a catalog install.
            cur.transition(&cur_path, AppState::Downloading)?;
            cur.transition(&cur_path, AppState::Verifying)?;
            self.step(&j, tx, "place")?;
            let vdir = self.paths.version_dir(id, &m.version)?;
            remove_tree(&vdir)?;
            copy_tree(dir, &vdir)?;
            let custom = m.state.as_ref().and_then(|s| s.dir.get(std::env::consts::OS)).is_some_and(|d| !d.is_empty());
            let meta = Meta { channel: LOCAL_CHANNEL.into(), state_schema: install::schema_of(m), previous_state_schema: None, custom_state_dir: custom, pending_update: None };
            store::write_json(&meta_path(&self.paths, id)?, &meta)?;
            cur.transition(&cur_path, AppState::Installed)?;
            if let Some(init) = &m.init {
                self.step(&j, tx, "init")?;
                cur.transition(&cur_path, AppState::Initialising)?;
                self.run_init(id, m, &vdir, &init.exec, &init.args)?;
            }
            cur.transition(&cur_path, AppState::Stopped)?;
            Ok(())
        })();
        match r {
            Ok(()) => {
                j.commit(tx)?;
                Ok(None)
            }
            Err(e) => {
                self.undo_install(id, &m.version);
                let _ = j.abort(tx, &e.to_string());
                Err(e)
            }
        }
    }

    /// Installed already: a different version goes through the update switch (rollback keeps working); the same
    /// version is replaced in place.
    fn local_replace(&self, dir: &Path, m: &Manifest, mut cur: Current, key: String) -> Result<Option<String>> {
        let id = m.id.as_str();
        if !matches!(cur.state, AppState::Stopped | AppState::Running | AppState::Installed | AppState::Failed(_)) {
            return Err(ArmoryError::WrongState(format!("app '{id}' is {}; it must be stopped or running to reinstall", cur.state.name())));
        }
        let cur_path = self.paths.current_json(id)?;
        let old = cur.version.clone();
        let was_running = cur.state == AppState::Running;
        let meta = self.load_meta(id).unwrap_or(Meta { channel: LOCAL_CHANNEL.into(), state_schema: 1, previous_state_schema: None, custom_state_dir: false, pending_update: None });
        let meta = Meta { channel: LOCAL_CHANNEL.into(), ..meta };
        let (j, tx) = self.begin_tx(id, "update", &format!("{old}|{}", m.version))?;
        if old == m.version {
            let r = (|| -> Result<()> {
                self.step(&j, tx, "stop")?;
                if was_running {
                    self.sup.stop(id).map_err(|e| ArmoryError::WrongState(format!("could not stop the app: {e}")))?;
                }
                self.step(&j, tx, "switch")?;
                let vdir = self.paths.version_dir(id, &old)?;
                remove_tree(&vdir)?;
                copy_tree(dir, &vdir)?;
                let c = self.load_current(id)?.unwrap_or(cur.clone());
                store::write_json(&cur_path, &Current { public_key: key.clone(), state: AppState::Stopped, ..c })?;
                store::write_json(&meta_path(&self.paths, id)?, &Meta { state_schema: install::schema_of(m), ..meta.clone() })?;
                if was_running {
                    self.step(&j, tx, "start")?;
                    self.sup.start(id).map_err(|e| ArmoryError::UpdateFailed { reason: e.to_string(), rolled_back: false })?;
                }
                Ok(())
            })();
            return match r {
                Ok(()) => {
                    j.commit(tx)?;
                    Ok(Some(old))
                }
                Err(e) => {
                    let _ = j.abort(tx, &e.to_string());
                    Err(e)
                }
            };
        }
        let vdir = self.paths.version_dir(id, &m.version)?;
        let placed = (|| -> Result<()> {
            self.step(&j, tx, "place")?;
            remove_tree(&vdir)?;
            copy_tree(dir, &vdir)
        })();
        if let Err(e) = placed {
            let _ = remove_tree(&vdir);
            let _ = j.abort(tx, &e.to_string());
            return Err(e);
        }
        cur.public_key = key;
        match self.switch_and_start(&j, tx, id, &mut cur, &cur_path, &old, &m.version, was_running, &meta, install::schema_of(m)) {
            Ok(()) => {
                j.commit(tx)?;
                self.prune_versions(id);
                Ok(Some(old))
            }
            Err(e) => {
                let _ = j.abort(tx, &e.to_string());
                Err(e)
            }
        }
    }
}

/// The channel name an app installed from a folder carries (no catalog feed has it, so a catalog update never
/// replaces it by accident).
pub const LOCAL_CHANNEL: &str = "local";

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let (p, t) = (e.path(), to.join(e.file_name()));
        if e.file_type()?.is_dir() {
            copy_tree(&p, &t)?;
        } else {
            std::fs::copy(&p, &t)?;
        }
    }
    Ok(())
}

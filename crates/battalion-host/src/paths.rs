// SPDX-License-Identifier: MPL-2.0
//! The on-disk layout, per user, no admin. `BATTALION_HOME` overrides the root (every test uses it).
//!
//! ```text
//! <home>/os.json  installed.json  journal.jsonl  os.lock  logs/  catalog/  staging/  bin/
//! <home>/apps/<id>/<version>/        immutable package contents
//! <home>/apps/<id>/current.json      {version, previous, state, publicKey}
//! <home>/apps/<id>/app.lock          one writer per app
//! <home>/data/<id>/                  default state dir
//! <home>/webview/<id>/               per-app web view data
//! ```

use battalion_app_api::manifest::valid_id;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadAppId(pub String);
impl std::fmt::Display for BadAppId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "app id '{}' is not allowed", self.0)
    }
}
impl std::error::Error for BadAppId {}

impl Paths {
    pub fn new(root: impl Into<PathBuf>) -> Paths {
        Paths { root: root.into() }
    }

    /// `BATTALION_HOME` if set and non-empty, else `%LOCALAPPDATA%\SuperiorSoft\BattalionOS`
    /// (Windows) or `~/.local/share/BattalionOS`.
    pub fn from_env() -> Option<Paths> {
        if let Some(h) = std::env::var_os(battalion_app_api::env::HOME).filter(|v| !v.is_empty()) {
            return Some(Paths::new(h));
        }
        #[cfg(windows)]
        {
            let base = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty())?;
            Some(Paths::new(PathBuf::from(base).join("SuperiorSoft").join("BattalionOS")))
        }
        #[cfg(not(windows))]
        {
            let home = std::env::var_os("HOME").filter(|v| !v.is_empty())?;
            Some(Paths::new(PathBuf::from(home).join(".local").join("share").join("BattalionOS")))
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn os_json(&self) -> PathBuf {
        self.root.join("os.json")
    }
    pub fn installed_json(&self) -> PathBuf {
        self.root.join("installed.json")
    }
    pub fn journal(&self) -> PathBuf {
        self.root.join("journal.jsonl")
    }
    /// Single-instance lock of the whole OS.
    pub fn os_lock(&self) -> PathBuf {
        self.root.join("os.lock")
    }
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }
    pub fn catalog_dir(&self) -> PathBuf {
        self.root.join("catalog")
    }
    pub fn staging(&self) -> PathBuf {
        self.root.join("staging")
    }
    pub fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }
    pub fn apps(&self) -> PathBuf {
        self.root.join("apps")
    }

    // Per app. The id is checked first: it becomes a folder name, so `..` never gets in.
    fn app(&self, id: &str) -> Result<PathBuf, BadAppId> {
        if valid_id(id) {
            Ok(self.apps().join(id))
        } else {
            Err(BadAppId(id.to_string()))
        }
    }
    pub fn app_dir(&self, id: &str) -> Result<PathBuf, BadAppId> {
        self.app(id)
    }
    pub fn version_dir(&self, id: &str, version: &str) -> Result<PathBuf, BadAppId> {
        if !battalion_app_api::Version::parse(version).is_some() {
            return Err(BadAppId(version.to_string()));
        }
        Ok(self.app(id)?.join(version))
    }
    pub fn current_json(&self, id: &str) -> Result<PathBuf, BadAppId> {
        Ok(self.app(id)?.join("current.json"))
    }
    pub fn app_lock(&self, id: &str) -> Result<PathBuf, BadAppId> {
        Ok(self.app(id)?.join("app.lock"))
    }
    /// Default state dir. A manifest that names its own `state.dir` overrides this; the OS never
    /// deletes a folder it did not create.
    pub fn data_dir(&self, id: &str) -> Result<PathBuf, BadAppId> {
        if valid_id(id) {
            Ok(self.root.join("data").join(id))
        } else {
            Err(BadAppId(id.to_string()))
        }
    }
    pub fn webview_dir(&self, id: &str) -> Result<PathBuf, BadAppId> {
        if valid_id(id) {
            Ok(self.root.join("webview").join(id))
        } else {
            Err(BadAppId(id.to_string()))
        }
    }

    /// Create the fixed folders.
    pub fn ensure(&self) -> std::io::Result<()> {
        for d in [self.root.clone(), self.logs(), self.catalog_dir(), self.staging(), self.bin(), self.apps()] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_cannot_escape() {
        let p = Paths::new("/h");
        assert!(p.app_dir("..").is_err());
        assert!(p.app_dir("a/b").is_err());
        assert!(p.data_dir("A").is_err());
        assert!(p.version_dir("ok-app", "../x").is_err());
        assert!(p.version_dir("ok-app", "1.0.0").unwrap().ends_with("1.0.0"));
    }
}

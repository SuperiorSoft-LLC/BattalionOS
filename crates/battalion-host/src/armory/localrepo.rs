// SPDX-License-Identifier: MPL-2.0
//! LOCAL-REPO-1: private local app repos. A local repo is a folder on this machine holding the same catalog v2 and
//! feed v1 files as a remote source; agents publish into it (an app publish command). Nothing is fetched from the
//! network: every url in a local repo is a `file:` url inside the repo's own folder.
//!
//! ```text
//! <home>/keys/local-repo.pub            this device's local key (the publisher makes it on first use)
//! <home>/local-repos/<slug>/repo.json   {"schema":1,"name":"My apps"}
//! <home>/local-repos/<slug>/catalog.json, apps/<id>/<channel>.json, apps/<id>/*.bosapp(.sig)
//! ```
//!
//! Folders under `<home>/local-repos/` are found on every read (add, rename and remove are folder operations, so
//! `os.json` is never rewritten). `os.json` `armory.localRepos` (`[{"name","path"}]`) may add folders kept elsewhere.
//!
//! Trust: an entry from a local repo is listed only when its `publicKey` is this device's local key, and every
//! channel url is a file inside the repo. The install then pins that key like any other, so a package signed by
//! anything else is refused by the usual signature check.

use super::*;

pub const LOCAL_LABEL: &str = "Local, built on this machine";
pub const DEFAULT_REPO: &str = "My apps";

/// One `os.json` `armory.localRepos` entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalRepoSetting {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalRepo {
    pub name: String,
    pub dir: PathBuf,
    /// Listed in os.json (kept elsewhere): rename and remove are done there, not here.
    pub registered: bool,
}

fn repos_root(paths: &Paths) -> PathBuf {
    paths.root().join("local-repos")
}

/// This device's local public key (the base64 line), if one was made.
pub fn device_key(paths: &Paths) -> Option<String> {
    let k = std::fs::read_to_string(paths.root().join("keys").join("local-repo.pub")).ok()?;
    let k = k.lines().map(str::trim).rfind(|l| !l.is_empty() && !l.starts_with("untrusted comment"))?.to_string();
    Some(k)
}

fn repo_name(dir: &Path) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("repo.json")).ok()?).ok()?;
    v.get("name")?.as_str().map(str::to_string).filter(|n| !n.trim().is_empty())
}

pub fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() { "repo".into() } else { s }
}

impl Armory {
    /// Every local repo: the folders under `<home>/local-repos/`, then os.json's extra ones.
    pub fn local_repos(&self) -> Vec<LocalRepo> {
        let mut out: Vec<LocalRepo> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(repos_root(&self.paths)) {
            let mut dirs: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
            dirs.sort();
            for d in dirs {
                if let Some(name) = repo_name(&d) {
                    out.push(LocalRepo { name, dir: d, registered: false });
                }
            }
        }
        for r in &self.settings.local_repos {
            if !out.iter().any(|o| o.name.eq_ignore_ascii_case(&r.name)) && !r.path.trim().is_empty() {
                out.push(LocalRepo { name: r.name.clone(), dir: PathBuf::from(&r.path), registered: true });
            }
        }
        out
    }

    fn local_repo(&self, name: &str) -> Result<LocalRepo> {
        self.local_repos().into_iter().find(|r| r.name.eq_ignore_ascii_case(name.trim())).ok_or_else(|| ArmoryError::LocalRepo(format!("no local repo named \"{}\"", name.trim())))
    }

    /// A new, empty local repo under `<home>/local-repos/`.
    pub fn add_local_repo(&self, name: &str) -> Result<LocalRepo> {
        let name = check_name(name)?;
        if self.local_repos().iter().any(|r| r.name.eq_ignore_ascii_case(&name)) {
            return Err(ArmoryError::LocalRepo(format!("a local repo named \"{name}\" already exists")));
        }
        let root = repos_root(&self.paths);
        let mut dir = root.join(slug(&name));
        let mut i = 2;
        while dir.exists() {
            dir = root.join(format!("{}-{i}", slug(&name)));
            i += 1;
        }
        let io = |e: std::io::Error| ArmoryError::LocalRepo(e.to_string());
        std::fs::create_dir_all(&dir).map_err(io)?;
        store::write_atomic(&dir.join("catalog.json"), b"{\n  \"schema\": 2,\n  \"apps\": []\n}\n").map_err(io)?;
        store::write_atomic(&dir.join("repo.json"), repo_json(&name).as_bytes()).map_err(io)?;
        Ok(LocalRepo { name, dir, registered: false })
    }

    /// Renames the repo (its repo.json); the folder and the apps installed from it stay as they are.
    pub fn rename_local_repo(&self, name: &str, new_name: &str) -> Result<LocalRepo> {
        let r = self.local_repo(name)?;
        let new_name = check_name(new_name)?;
        if r.registered {
            return Err(ArmoryError::LocalRepo(format!("\"{}\" is listed in os.json (armory.localRepos); rename it there", r.name)));
        }
        if self.local_repos().iter().any(|o| o.dir != r.dir && o.name.eq_ignore_ascii_case(&new_name)) {
            return Err(ArmoryError::LocalRepo(format!("a local repo named \"{new_name}\" already exists")));
        }
        store::write_atomic(&r.dir.join("repo.json"), repo_json(&new_name).as_bytes()).map_err(|e| ArmoryError::LocalRepo(e.to_string()))?;
        Ok(LocalRepo { name: new_name, ..r })
    }

    /// Deletes the repo's folder (its packages). Apps installed from it stay installed.
    pub fn remove_local_repo(&self, name: &str) -> Result<()> {
        let r = self.local_repo(name)?;
        if r.registered {
            return Err(ArmoryError::LocalRepo(format!("\"{}\" is listed in os.json (armory.localRepos); remove it there", r.name)));
        }
        // only ever a folder under <home>/local-repos/
        if r.dir.parent() != Some(repos_root(&self.paths).as_path()) {
            return Err(ArmoryError::LocalRepo("refusing to delete a folder outside the local repos folder".into()));
        }
        std::fs::remove_dir_all(&r.dir).map_err(|e| ArmoryError::LocalRepo(e.to_string()))
    }

    /// The entries of every local repo, checked (see the module doc). Rejected ones go to `rejected`.
    pub(crate) fn local_catalog(&self, out: &mut Listing, seen: &mut std::collections::BTreeSet<String>) {
        let repos = self.local_repos();
        if repos.is_empty() {
            return;
        }
        let key = device_key(&self.paths);
        for r in repos {
            let source = format!("local:{}", r.name);
            let text = match std::fs::read(r.dir.join("catalog.json")) {
                Ok(b) if b.len() as u64 <= self.settings.max_catalog_bytes => String::from_utf8_lossy(&b).to_string(),
                Ok(_) => {
                    out.source_errors.push((source, "catalog.json is too large".into()));
                    continue;
                }
                Err(e) => {
                    out.source_errors.push((source, e.to_string()));
                    continue;
                }
            };
            let (apps, bad) = match Catalog::parse_lenient(&text) {
                Ok(p) => p,
                Err(e) => {
                    out.source_errors.push((source, e.to_string()));
                    continue;
                }
            };
            out.rejected.extend(bad.into_iter().map(|(id, e)| (id, e.to_string())));
            for app in apps {
                if key.as_deref().map(str::trim) != Some(app.public_key.trim()) {
                    out.rejected.push((app.id.clone(), format!("{source}: not signed with this device's local key")));
                    continue;
                }
                if let Some(u) = app.channels.values().find(|u| !inside(&r.dir, u)) {
                    out.rejected.push((app.id.clone(), format!("{source}: channel {u} is not a file in the repo")));
                    continue;
                }
                if !seen.insert(app.id.clone()) {
                    out.rejected.push((app.id.clone(), format!("already listed by an earlier source (not {source})")));
                    continue;
                }
                let compatible = app.contract.min <= CONTRACT && CONTRACT <= app.contract.max;
                out.apps.push(CatalogEntry { app, source: source.clone(), compatible, from_local_repo: true });
            }
        }
    }
}

use battalion_app_api::{Catalog, CONTRACT};

fn check_name(name: &str) -> Result<String> {
    let n = name.trim();
    if n.is_empty() || n.len() > 64 || n.chars().any(|c| c.is_control()) {
        return Err(ArmoryError::LocalRepo("a repo name is 1 to 64 characters".into()));
    }
    Ok(n.to_string())
}

fn repo_json(name: &str) -> String {
    serde_json::to_string_pretty(&serde_json::json!({"schema": 1, "name": name})).unwrap() + "\n"
}

/// A `file:` url naming a file inside `dir` (no `..`).
fn inside(dir: &Path, url: &str) -> bool {
    let Some(p) = file_url_path(url) else { return false };
    if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return false;
    }
    let norm = |x: &Path| x.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase();
    let (d, f) = (norm(dir), norm(&p));
    f.starts_with(&(d + "/"))
}

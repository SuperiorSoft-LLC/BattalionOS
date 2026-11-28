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
//! ARMORY-LOCAL-2, linked repos: `repo.json` may carry `"path"`, a folder elsewhere on this machine (a local git
//! checkout of a repo shaped like battalionOS-apps: catalog.json plus feeds and packages). The link folder under
//! `<home>/local-repos/` holds only that repo.json; removing the repo removes the link, never the folder it names.
//! A linked repo's entries carry their own publisher key (as a `file:` catalog source does); channel and package urls
//! may be paths relative to the catalog or feed file.
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
    /// ARMORY-LOCAL-2: the link folder under `<home>/local-repos/` when `dir` is a folder elsewhere.
    pub link: Option<PathBuf>,
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

/// The name and, for a linked repo, the folder it names.
fn repo_info(dir: &Path) -> Option<(String, Option<PathBuf>)> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("repo.json")).ok()?).ok()?;
    let name = v.get("name")?.as_str().map(str::to_string).filter(|n| !n.trim().is_empty())?;
    let path = v.get("path").and_then(|p| p.as_str()).filter(|p| !p.trim().is_empty()).map(PathBuf::from);
    Some((name, path))
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
                match repo_info(&d) {
                    Some((name, Some(target))) => out.push(LocalRepo { name, dir: target, registered: false, link: Some(d) }),
                    Some((name, None)) => out.push(LocalRepo { name, dir: d, registered: false, link: None }),
                    None => {}
                }
            }
        }
        for r in &self.settings.local_repos {
            if !out.iter().any(|o| o.name.eq_ignore_ascii_case(&r.name)) && !r.path.trim().is_empty() {
                out.push(LocalRepo { name: r.name.clone(), dir: PathBuf::from(&r.path), registered: true, link: None });
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
        Ok(LocalRepo { name, dir, registered: false, link: None })
    }

    /// ARMORY-LOCAL-2: list a folder kept elsewhere (a local checkout holding catalog.json) as a local repo.
    pub fn link_local_repo(&self, name: &str, folder: &Path) -> Result<LocalRepo> {
        let name = check_name(name)?;
        if !folder.join("catalog.json").is_file() {
            return Err(ArmoryError::LocalRepo(format!("{} has no catalog.json", folder.display())));
        }
        let repos = self.local_repos();
        if repos.iter().any(|r| r.name.eq_ignore_ascii_case(&name)) {
            return Err(ArmoryError::LocalRepo(format!("a local repo named \"{name}\" already exists")));
        }
        if repos.iter().any(|r| r.dir == folder) {
            return Err(ArmoryError::LocalRepo(format!("{} is already a local repo", folder.display())));
        }
        let root = repos_root(&self.paths);
        let mut link = root.join(slug(&name));
        let mut i = 2;
        while link.exists() {
            link = root.join(format!("{}-{i}", slug(&name)));
            i += 1;
        }
        let io = |e: std::io::Error| ArmoryError::LocalRepo(e.to_string());
        std::fs::create_dir_all(&link).map_err(io)?;
        store::write_atomic(&link.join("repo.json"), linked_json(&name, folder).as_bytes()).map_err(io)?;
        Ok(LocalRepo { name, dir: folder.to_path_buf(), registered: false, link: Some(link) })
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
        let text = match &r.link {
            Some(_) => linked_json(&new_name, &r.dir),
            None => repo_json(&new_name),
        };
        store::write_atomic(&r.link.as_ref().unwrap_or(&r.dir).join("repo.json"), text.as_bytes()).map_err(|e| ArmoryError::LocalRepo(e.to_string()))?;
        Ok(LocalRepo { name: new_name, ..r })
    }

    /// Deletes the repo's folder (its packages). Apps installed from it stay installed.
    pub fn remove_local_repo(&self, name: &str) -> Result<()> {
        let r = self.local_repo(name)?;
        if r.registered {
            return Err(ArmoryError::LocalRepo(format!("\"{}\" is listed in os.json (armory.localRepos); remove it there", r.name)));
        }
        if let Some(link) = &r.link {
            // A linked repo: only the link goes; the folder it names is the owner's.
            if link.parent() != Some(repos_root(&self.paths).as_path()) {
                return Err(ArmoryError::LocalRepo("refusing to delete a folder outside the local repos folder".into()));
            }
            return std::fs::remove_dir_all(link).map_err(|e| ArmoryError::LocalRepo(e.to_string()));
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
            let text = resolve_relative(&text, &r.dir, &["apps", "*", "channels", "*"]);
            let (apps, bad) = match Catalog::parse_lenient(&text) {
                Ok(p) => p,
                Err(e) => {
                    out.source_errors.push((source, e.to_string()));
                    continue;
                }
            };
            out.rejected.extend(bad.into_iter().map(|(id, e)| (id, e.to_string())));
            for app in apps {
                // A linked repo's entries carry their publisher's key; the install pins it like any catalog's.
                if r.link.is_none() && key.as_deref().map(str::trim) != Some(app.public_key.trim()) {
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

fn linked_json(name: &str, folder: &Path) -> String {
    serde_json::to_string_pretty(&serde_json::json!({"schema": 1, "name": name, "path": folder.display().to_string()})).unwrap() + "\n"
}

/// ARMORY-LOCAL-2: turn the relative paths at `at` (a json path; `*` = every element or value) into `file:` urls
/// under `base`. Text that is not json, or has no relative path there, comes back as it was.
pub(crate) fn resolve_relative(text: &str, base: &Path, at: &[&str]) -> String {
    fn walk(v: &mut serde_json::Value, at: &[&str], base: &Path, changed: &mut bool) {
        let Some((first, rest)) = at.split_first() else {
            if let Some(s) = v.as_str() {
                if !s.is_empty() && !s.contains(':') && !s.starts_with('/') && !s.starts_with('\\') {
                    let p = base.join(s).display().to_string().replace('\\', "/");
                    *v = (if p.starts_with('/') { format!("file://{p}") } else { format!("file:///{p}") }).into();
                    *changed = true;
                }
            }
            return;
        };
        match (*first, v) {
            ("*", serde_json::Value::Array(a)) => a.iter_mut().for_each(|x| walk(x, rest, base, changed)),
            ("*", serde_json::Value::Object(o)) => o.values_mut().for_each(|x| walk(x, rest, base, changed)),
            (k, serde_json::Value::Object(o)) => {
                if let Some(x) = o.get_mut(k) {
                    walk(x, rest, base, changed)
                }
            }
            _ => {}
        }
    }
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(text) else { return text.to_string() };
    let mut changed = false;
    walk(&mut v, at, base, &mut changed);
    if changed { v.to_string() } else { text.to_string() }
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

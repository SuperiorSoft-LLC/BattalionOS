// SPDX-License-Identifier: MPL-2.0
//! Developer mode (BOS-31): an app taken from a folder, with no package, signature, catalog or install.
//!
//! This exists only in a build with the cargo feature `dev` (compile time, not a setting). A release build has no
//! `register`, no watcher, and [`load`] answers `None` for every app, so a folder app that somehow has a `dev.json`
//! is not run: its content dir is the (missing) version dir. Knows no app by name.
//!
//! Data: `apps/<id>/dev.json` = `{"dir": <app folder>, "devUrl": <loopback url of the app's own dev server, optional>}`.
//! The app folder holds `app.json`, the built library and the page folder, as in a package.

use crate::paths::Paths;
use serde::{Deserialize, Serialize};

/// Is this build a developer build?
pub const ENABLED: bool = cfg!(feature = "dev");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevApp {
    pub dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_url: Option<String>,
    /// Environment the dev app is started with, on top of its manifest's own (e.g. a free port the developer chose). Data only.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub env: std::collections::BTreeMap<String, String>,
}

fn dev_json(paths: &Paths, id: &str) -> Option<std::path::PathBuf> {
    paths.app_dir(id).ok().map(|d| d.join("dev.json"))
}

/// The dev registration of app `id`. Always `None` in a build without the `dev` feature.
pub fn load(paths: &Paths, id: &str) -> Option<DevApp> {
    if !ENABLED {
        return None;
    }
    crate::store::read_json::<DevApp>(&dev_json(paths, id)?).ok().flatten()
}

/// `http://127.0.0.1:5173/x` -> `http://127.0.0.1:5173`. Only loopback (`127.0.0.1`, `localhost`, `[::1]`), only http(s), no
/// credentials: the frame loads this url, so it must not be anywhere else. Pure, so it is checked in every build.
pub fn dev_url_origin(url: &str) -> Result<String, String> {
    let (scheme, rest) = url.split_once("://").ok_or("the dev url needs a scheme (http://127.0.0.1:<port>)")?;
    if scheme != "http" && scheme != "https" {
        return Err("the dev url must be http or https".into());
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains(['@', '\\', ' ']) {
        return Err("the dev url has no usable host".into());
    }
    let host = if let Some(h) = authority.strip_prefix('[') { h.split(']').next().unwrap_or("").to_string() } else { authority.split(':').next().unwrap_or("").to_string() };
    if !matches!(host.to_ascii_lowercase().as_str(), "127.0.0.1" | "localhost" | "::1") {
        return Err(format!("the dev url must be on this PC's loopback (127.0.0.1, localhost or [::1]), not '{host}'"));
    }
    Ok(format!("{scheme}://{authority}"))
}

impl Paths {
    /// The folder app `id` runs from: its dev folder when it is a dev app, else its installed version folder.
    pub fn content_dir(&self, id: &str, version: &str) -> Result<std::path::PathBuf, crate::paths::BadAppId> {
        match load(self, id) {
            Some(d) => Ok(d.dir.into()),
            None => self.version_dir(id, version),
        }
    }
}

/// The library the worker loads. A dev app's library is COPIED first (Windows locks a loaded library, and the compiler must be free to
/// overwrite the original); the copy lives in the app's run folder under a name not used before. Anything else is loaded as it is.
pub fn library_to_load(paths: &Paths, id: &str, original: &std::path::Path) -> Result<std::path::PathBuf, String> {
    if load(paths, id).is_none() {
        return Ok(original.to_path_buf());
    }
    let dir = paths.root().join("run").join(id).join("dev-lib");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    // Copies of earlier starts: one a running worker still holds cannot be deleted yet (ignored), the rest go.
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let name = original.file_name().and_then(|n| n.to_str()).unwrap_or("app.lib");
    let copy = dir.join(format!("{}-{n}-{name}", std::process::id()));
    std::fs::copy(original, &copy).map_err(|e| format!("cannot copy the library {} (is it built?): {e}", original.display()))?;
    Ok(copy)
}

#[cfg(feature = "dev")]
pub use dev_only::*;

#[cfg(feature = "dev")]
mod dev_only {
    use super::*;
    use crate::state::{AppState, Current};
    use crate::supervisor::{platform_key, Supervisor};
    use battalion_app_api::{Manifest, CONTRACT};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// Register the app in `folder` (its `app.json` names it). No package, signature, catalog or install: the app runs from the
    /// folder. An app that is installed the normal way is never replaced. Returns the app id.
    pub fn register(paths: &Paths, folder: &Path, dev_url: Option<&str>, env: &std::collections::BTreeMap<String, String>) -> Result<String, String> {
        if let Some(k) = env.keys().find(|k| k.is_empty() || k.contains(['=', '\0']) || k.to_ascii_uppercase().starts_with("BATTALION_")) {
            return Err(format!("dev env name '{k}' is not allowed (the OS's own BATTALION_* values are not set from outside)"));
        }
        let dir = folder.canonicalize().map_err(|e| format!("{}: {e}", folder.display()))?;
        let dir = PathBuf::from(dir.to_string_lossy().trim_start_matches(r"\\?\"));
        let text = std::fs::read_to_string(dir.join("app.json")).map_err(|e| format!("no app.json in {}: {e}", dir.display()))?;
        let m = Manifest::parse(&text, &platform_key(), CONTRACT).map_err(|e| format!("app.json: {e:?}"))?;
        // A page-only app (a page, no process part) runs as its page, like an installed one; it has no library to watch.
        if m.process_for(&platform_key()).is_none() && m.ui.is_none() {
            return Err(format!("app.json has no process part for {} and no page", platform_key()));
        }
        let url = match dev_url {
            Some(u) => {
                dev_url_origin(u)?;
                if m.ui.is_none() {
                    return Err("a dev url needs the app to have a ui".into());
                }
                Some(u.trim_end_matches('/').to_string())
            }
            None => None,
        };
        paths.ensure().map_err(|e| e.to_string())?;
        let id = m.id.clone();
        let app_dir = paths.app_dir(&id).map_err(|e| e.0)?;
        std::fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;
        let cur_path = paths.current_json(&id).map_err(|e| e.0)?;
        let existing = Current::load(&cur_path).map_err(|e| e.to_string())?;
        if existing.is_some() && load(paths, &id).is_none() {
            return Err(format!("app '{id}' is installed the normal way in this data folder; dev mode does not replace it"));
        }
        crate::store::write_json(&app_dir.join("dev.json"), &DevApp { dir: dir.display().to_string(), dev_url: url, env: env.clone() }).map_err(|e| e.to_string())?;
        // A registration is always stopped: the OS of this run starts it.
        let cur = Current { version: m.version.clone(), previous: None, state: AppState::Stopped, public_key: String::new() };
        crate::store::write_json(&cur_path, &cur).map_err(|e| e.to_string())?;
        Ok(id)
    }

    /// Every registered dev app: (id, library file).
    pub fn apps(paths: &Paths) -> Vec<(String, PathBuf)> {
        let Ok(rd) = std::fs::read_dir(paths.apps()) else { return vec![] };
        let mut out = Vec::new();
        for e in rd.flatten() {
            let id = e.file_name().to_string_lossy().to_string();
            let Some(d) = load(paths, &id) else { continue };
            let Ok(text) = std::fs::read_to_string(Path::new(&d.dir).join("app.json")) else { continue };
            let Ok(m) = Manifest::parse(&text, &platform_key(), CONTRACT) else { continue };
            if let Some(p) = m.process_for(&platform_key()) {
                out.push((id, Path::new(&d.dir).join(&p.library)));
            }
        }
        out
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum DevEvent {
        /// The library changed and the new one runs; `ms` from the change seen to ready.
        Restarted { id: String, ms: u64 },
        /// The new library did not start; the message is plain. The next change tries again.
        Failed { id: String, error: String },
    }

    pub struct Watcher {
        stop: Arc<AtomicBool>,
    }
    impl Drop for Watcher {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    type Stamp = Option<(std::time::SystemTime, u64)>;
    fn stamp(p: &Path) -> Stamp {
        let m = std::fs::metadata(p).ok()?;
        Some((m.modified().ok()?, m.len()))
    }

    /// Watch every dev app's library. When it changes (and stops changing: a linker writes in steps) the app is stopped the normal way
    /// and started again from a copy of the new file; the OS and other apps are not touched. A failed load is reported and the watching
    /// goes on. An app the developer stopped is left stopped.
    pub fn watch(paths: Paths, sup: Supervisor, poll: Duration, on_event: Arc<dyn Fn(DevEvent) + Send + Sync>) -> Watcher {
        let stop = Arc::new(AtomicBool::new(false));
        let st = stop.clone();
        std::thread::Builder::new()
            .name("bos-dev-watch".into())
            .spawn(move || {
                let mut seen: std::collections::HashMap<String, Stamp> = Default::default();
                // Apps whose last reload did not start: a failed start leaves them stopped, and the next good build must start them.
                let mut broken: std::collections::HashSet<String> = Default::default();
                while !st.load(Ordering::Relaxed) {
                    for (id, lib) in apps(&paths) {
                        let now = stamp(&lib);
                        let Some(before) = seen.get(&id).cloned() else {
                            seen.insert(id, now);
                            continue;
                        };
                        if now == before || now.is_none() {
                            continue;
                        }
                        let t0 = Instant::now();
                        let mut cur = now;
                        loop {
                            std::thread::sleep(poll);
                            let again = stamp(&lib);
                            let settled = again == cur;
                            cur = again;
                            if settled || st.load(Ordering::Relaxed) {
                                break;
                            }
                        }
                        seen.insert(id.clone(), cur);
                        let state = sup.status(&id).map(|s| s.state).unwrap_or_default();
                        if state != "running" && state != "failed" && !broken.contains(&id) {
                            continue;
                        }
                        let r = sup.stop(&id).map_err(|e| e.to_string()).and_then(|_| sup.start(&id).map_err(|e| e.to_string()));
                        if r.is_ok() {
                            broken.remove(&id);
                        } else {
                            broken.insert(id.clone());
                        }
                        on_event(match r {
                            Ok(_) => DevEvent::Restarted { id, ms: t0.elapsed().as_millis() as u64 },
                            Err(error) => DevEvent::Failed { id, error },
                        });
                    }
                    std::thread::sleep(poll);
                }
            })
            .expect("dev watcher thread");
        Watcher { stop }
    }
}

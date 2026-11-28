// SPDX-License-Identifier: MPL-2.0
//! Update with rollback.
//!
//! ```text
//! (the old version keeps running while the new one downloads, is checked and is placed: it is staged)
//! steps: download, verify, unpack, place, [prepare], stop, switch, start, done      then commit
//! ```
//! UPDATE-LIVE-1, `prepare`: a running app that names `prepare` in its ready file is asked to get ready (it stops starting
//! new work and lets work in progress finish) and asked again every `preparePollMs` until it answers ready; then it is
//! stopped. An app without `prepare` is waited for until its health says `busy` 0. When `prepareWaitMs` passes first, the
//! app is told to carry on (`cancel`), the staged version stays and the update answers `NotReady`; the caller offers
//! "Update now" (`now`: the app checkpoints at once) or "Later" ([Armory::defer_update]: applied by [Armory::apply_pending]
//! at the app's next start or next idle moment). A staged version folder is reused by the next try (it was verified when placed).
//! `switch` writes `current.json` as `{version: new, previous: old, state: updating}`. The new
//! version has to become ready (the supervisor's `start`). If it does not:
//! - same state schema: `current.json` goes back to the old version and it is started again,
//!   so the old version answers; the update reports `UpdateFailed { rolled_back: true }`;
//! - the state schema went up: the new version may already have changed the state, so the old
//!   version is not started on top of it. The app is left `failed` with the reason, the old
//!   version stays on disk, and a person decides.

use super::install::schema_of;
use super::*;
use crate::state::{AppState, Current};
use battalion_app_api::Version;

#[derive(Debug, Clone, Default)]
pub struct UpdateOptions {
    /// Another channel than the one the app was installed on.
    pub channel: Option<String>,
    /// Update without asking the app (no prepare, no busy check).
    pub force: bool,
    /// Ask the app to checkpoint now ("Update now"): `prepare` with `now`, so it answers ready at once.
    pub now: bool,
    /// How long to wait for the app to get ready; `None` = the setting `prepareWaitMs`. `Some(0)` = ask once.
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Updated {
    pub id: String,
    pub from: String,
    pub to: String,
}

/// What the catalog offers against what is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Available {
    pub id: String,
    pub installed: String,
    pub available: String,
    pub newer: bool,
    /// The feed came from the cache (offline).
    pub stale: bool,
}

impl Armory {
    /// Is a newer version offered? Downloads nothing.
    pub fn check_update(&self, id: &str, channel: Option<&str>) -> Result<Available> {
        let cur = self.load_current(id)?.ok_or_else(|| ArmoryError::NotInstalled(id.into()))?;
        let entry = self.find(id)?;
        let channel = channel.map(str::to_string).or_else(|| self.load_meta(id).map(|m| m.channel)).unwrap_or_else(|| "stable".into());
        let (feed, stale) = self.feed(&entry.app, &channel)?;
        let newer = match (Version::parse(&feed.version), Version::parse(&cur.version)) {
            (Some(n), Some(o)) => n > o,
            _ => false,
        };
        Ok(Available { id: id.into(), installed: cur.version, available: feed.version, newer, stale })
    }

    pub fn update(&self, id: &str, opts: &UpdateOptions) -> Result<Updated> {
        self.paths.app_dir(id)?;
        let _lock = self.lock_app(id)?;
        let cur_path = self.paths.current_json(id)?;
        let mut cur = self.load_current(id)?.ok_or_else(|| ArmoryError::NotInstalled(id.into()))?;
        if !matches!(cur.state, AppState::Stopped | AppState::Running | AppState::Installed) {
            return Err(ArmoryError::WrongState(format!("app '{id}' is {}; it must be stopped or running to update", cur.state.name())));
        }
        let meta = self.load_meta(id).unwrap_or(Meta { channel: "stable".into(), state_schema: 1, previous_state_schema: None, custom_state_dir: false, pending_update: None });
        let channel = opts.channel.clone().unwrap_or_else(|| meta.channel.clone());
        let entry = self.find(id)?;
        let key = entry.app.public_key.trim().to_string();
        if !cur.public_key.is_empty() && cur.public_key.trim() != key {
            return Err(ArmoryError::KeyChanged);
        }
        let (feed, _stale) = self.feed(&entry.app, &channel)?;
        let (old, new) = (cur.version.clone(), feed.version.clone());
        match (Version::parse(&new), Version::parse(&old)) {
            (Some(n), Some(o)) if n > o => {}
            _ => return Err(ArmoryError::UpToDate(old)),
        }
        let plat = self.plan(&entry.app, &feed)?;
        let was_running = cur.state == AppState::Running;

        let (j, tx) = self.begin_tx(id, "update", &format!("{old}|{new}"))?;
        let vdir = self.paths.version_dir(id, &new)?;
        // Everything up to `prepare` leaves the running app alone. A version placed by an earlier try is used as it is.
        let manifest = match self.staged_manifest(id, &new) {
            Some(m) => m,
            None => {
                let staged = self.fetch_and_check(&entry.app, &feed, &plat, &key, "update", &mut |s| self.step(&j, tx, s));
                let (dir, manifest) = match staged {
                    Ok(x) => x,
                    Err(e) => {
                        let _ = j.abort(tx, &e.to_string());
                        return Err(e);
                    }
                };
                let placed = (|| -> Result<()> {
                    self.step(&j, tx, "place")?;
                    remove_tree(&vdir)?;
                    std::fs::rename(&dir, &vdir)?;
                    Ok(())
                })();
                if let Err(e) = placed {
                    let _ = remove_tree(&dir);
                    let _ = j.abort(tx, &e.to_string());
                    return Err(e);
                }
                manifest
            }
        };
        let new_schema = schema_of(&manifest);
        if was_running && self.settings.check_busy && !opts.force {
            if let Err(e) = self.wait_ready(id, &new, opts) {
                // The staged version is kept for the next try; the journal holds nothing open for it.
                let _ = j.abort(tx, &e.to_string());
                return Err(e);
            }
        }

        let r = self.switch_and_start(&j, tx, id, &mut cur, &cur_path, &old, &new, was_running, &meta, new_schema);
        match r {
            Ok(()) => {
                self.point("commit");
                j.commit(tx)?;
                self.prune_versions(id);
                Ok(Updated { id: id.into(), from: old, to: new })
            }
            Err(e) => {
                let _ = j.abort(tx, &e.to_string());
                Err(e)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn switch_and_start(&self, j: &crate::journal::Journal, tx: u64, id: &str, cur: &mut Current, cur_path: &Path, old: &str, new: &str, was_running: bool, meta: &Meta, new_schema: u32) -> Result<()> {
        let vdir_new = self.paths.version_dir(id, new)?;
        let give_up = |e: ArmoryError| {
            // Nothing switched yet: the old version is intact.
            let _ = remove_tree(&vdir_new);
            e
        };
        self.step(j, tx, "stop")?;
        if was_running {
            self.sup.stop(id).map_err(|e| give_up(ArmoryError::WrongState(format!("could not stop the app: {e}"))))?;
            *cur = self.load_current(id)?.ok_or_else(|| ArmoryError::NotInstalled(id.into()))?;
        }
        self.step(j, tx, "switch")?;
        let orig_previous = cur.previous.clone();
        cur.previous = Some(old.to_string());
        cur.version = new.to_string();
        cur.transition(cur_path, AppState::Updating).map_err(|e| {
            cur.version = old.to_string();
            cur.previous = orig_previous.clone();
            give_up(e.into())
        })?;
        let new_meta = Meta { state_schema: new_schema, previous_state_schema: Some(meta.state_schema), pending_update: None, ..meta.clone() };
        store::write_json(&meta_path(&self.paths, id)?, &new_meta)?;
        cur.transition(cur_path, AppState::Stopped)?;

        self.step(j, tx, "start")?;
        match self.sup.start(id) {
            Ok(_) => {
                self.step(j, tx, "done")?;
                if !was_running {
                    // It only had to prove it answers.
                    let _ = self.sup.stop(id);
                }
                Ok(())
            }
            Err(start_err) => {
                let reason = start_err.to_string();
                if new_schema != meta.state_schema {
                    let mut c = self.load_current(id)?.unwrap_or_else(|| cur.clone());
                    let _ = c.transition(cur_path, AppState::Failed(format!("update to {new} never got ready: {reason}")));
                    return Err(ArmoryError::UpdateFailed { reason: format!("{reason} ({})", ArmoryError::SchemaBump { from: meta.state_schema, to: new_schema }), rolled_back: false });
                }
                self.roll_back_files(id, cur_path, old, orig_previous, meta)?;
                let rolled_back = if was_running {
                    match self.sup.start(id) {
                        Ok(_) => true,
                        Err(e) => {
                            if let Ok(Some(mut c)) = self.load_current(id) {
                                let _ = c.transition(cur_path, AppState::Failed(format!("rolled back to {old} but it did not start: {e}")));
                            }
                            false
                        }
                    }
                } else {
                    true
                };
                if rolled_back {
                    let _ = remove_tree(&vdir_new);
                }
                Err(ArmoryError::UpdateFailed { reason, rolled_back })
            }
        }
    }

    /// The manifest of version `version` when it is already placed (staged by an earlier try) and still checks out.
    fn staged_manifest(&self, id: &str, version: &str) -> Option<battalion_app_api::Manifest> {
        let vdir = self.paths.version_dir(id, version).ok()?;
        if !vdir.join("app.json").is_file() {
            return None;
        }
        self.check_manifest(&vdir, id, version).ok()
    }

    /// Asks the running app to get ready for the update to `to` until it is, or the wait ends (then `NotReady`, and the
    /// app is told to carry on). Each answer that is not ready is reported as a `prepare` progress.
    pub(crate) fn wait_ready(&self, id: &str, to: &str, opts: &UpdateOptions) -> Result<()> {
        use battalion_app_api::PrepareRequest;
        let wait = std::time::Duration::from_millis(opts.wait_ms.unwrap_or(self.settings.prepare_wait_ms));
        let poll = std::time::Duration::from_millis(self.settings.prepare_poll_ms.max(50));
        let started = std::time::Instant::now();
        let ask = PrepareRequest { to: Some(to.to_string()), now: opts.now, cancel: false };
        let mut asked = false;
        loop {
            self.point("prepare");
            let (reason, eta_ms) = match self.sup.prepare(id, &ask) {
                Ok(Some(a)) if a.ready => return Ok(()),
                Ok(Some(a)) => {
                    asked = true;
                    (a.reason.filter(|r| !r.trim().is_empty()).unwrap_or_else(|| "it finishes its current work".into()), a.eta_ms)
                }
                // No `prepare`: the app is ready when its health says nothing is busy.
                Ok(None) => match self.sup.health(id).map(|h| h.busy.unwrap_or(0)) {
                    Ok(0) | Err(_) => return Ok(()),
                    Ok(n) => (format!("its {n} running {} finish", if n == 1 { "task" } else { "tasks" }), None),
                },
                // It names `prepare` and gives no valid answer: still running, so it is asked again.
                Err(e) => {
                    asked = true;
                    (format!("it answers (it did not: {e})"), None)
                }
            };
            if started.elapsed() >= wait {
                if asked {
                    let _ = self.sup.prepare(id, &PrepareRequest { cancel: true, ..Default::default() });
                }
                return Err(ArmoryError::NotReady { version: to.to_string(), reason, eta_ms });
            }
            self.report(Progress::waiting(id, &reason, eta_ms));
            std::thread::sleep(poll.min(wait.saturating_sub(started.elapsed())).max(std::time::Duration::from_millis(10)));
        }
    }

    /// "Later": the staged (or offered) newer version is applied by [Armory::apply_pending] at the app's next start or next
    /// idle moment. The app is told to carry on with its work.
    pub fn defer_update(&self, id: &str) -> Result<String> {
        let a = self.check_update(id, None)?;
        if !a.newer {
            return Err(ArmoryError::UpToDate(a.installed));
        }
        let _lock = self.lock_app(id)?;
        let mut meta = self.load_meta(id).ok_or_else(|| ArmoryError::NotInstalled(id.into()))?;
        meta.pending_update = Some(a.available.clone());
        store::write_json(&meta_path(&self.paths, id)?, &meta)?;
        let _ = self.sup.prepare(id, &battalion_app_api::PrepareRequest { cancel: true, ..Default::default() });
        Ok(a.available)
    }

    /// The version "Later" was chosen for, if any.
    pub fn pending_update(&self, id: &str) -> Option<String> {
        self.load_meta(id).and_then(|m| m.pending_update)
    }

    /// Applies a pending update ("Later") when the moment is right: the app is not running (its next start), or it is
    /// idle now (it answers ready at once). `Ok(None)` = nothing pending, or the app is busy (it stays pending).
    pub fn apply_pending(&self, id: &str) -> Result<Option<Updated>> {
        if self.pending_update(id).is_none() {
            return Ok(None);
        }
        match self.update(id, &UpdateOptions { wait_ms: Some(0), ..Default::default() }) {
            Ok(u) => Ok(Some(u)),
            Err(ArmoryError::NotReady { .. }) | Err(ArmoryError::Busy) => Ok(None),
            Err(ArmoryError::UpToDate(_)) => {
                if let Some(mut m) = self.load_meta(id) {
                    m.pending_update = None;
                    let _ = store::write_json(&meta_path(&self.paths, id)?, &m);
                }
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// `current.json` and the sidecar go back to `old`, stopped.
    pub(crate) fn roll_back_files(&self, id: &str, cur_path: &Path, old: &str, previous: Option<String>, meta: &Meta) -> Result<()> {
        let key = self.load_current(id)?.map(|c| c.public_key).unwrap_or_default();
        let back = Current { version: old.to_string(), previous, state: AppState::Stopped, public_key: key };
        store::write_json(cur_path, &back)?;
        store::write_json(&meta_path(&self.paths, id)?, meta)?;
        Ok(())
    }

    /// Manual rollback to the previous version. Refused when the state schema changed between
    /// the two (the new version may have changed the state), and when the old files are gone.
    pub fn rollback(&self, id: &str) -> Result<Updated> {
        self.paths.app_dir(id)?;
        let _lock = self.lock_app(id)?;
        let cur_path = self.paths.current_json(id)?;
        let cur = self.load_current(id)?.ok_or_else(|| ArmoryError::NotInstalled(id.into()))?;
        let prev = cur.previous.clone().ok_or(ArmoryError::NoPrevious)?;
        if !self.paths.version_dir(id, &prev)?.join("app.json").is_file() {
            return Err(ArmoryError::NoPrevious);
        }
        let meta = self.load_meta(id).ok_or(ArmoryError::NoPrevious)?;
        let prev_schema = meta.previous_state_schema.ok_or(ArmoryError::NoPrevious)?;
        if prev_schema != meta.state_schema {
            return Err(ArmoryError::SchemaBump { from: prev_schema, to: meta.state_schema });
        }
        let was_running = cur.state == AppState::Running;
        if !matches!(cur.state, AppState::Stopped | AppState::Running | AppState::Failed(_)) {
            return Err(ArmoryError::WrongState(format!("app '{id}' is {}", cur.state.name())));
        }
        self.report(Progress::stage(id, "rollback", "stop"));
        if was_running {
            self.sup.stop(id).map_err(|e| ArmoryError::WrongState(e.to_string()))?;
        }
        self.report(Progress::stage(id, "rollback", "switch"));
        let back = Current { version: prev.clone(), previous: Some(cur.version.clone()), state: AppState::Stopped, public_key: cur.public_key.clone() };
        store::write_json(&cur_path, &back)?;
        store::write_json(&meta_path(&self.paths, id)?, &Meta { previous_state_schema: Some(meta.state_schema), ..meta })?;
        if was_running {
            self.report(Progress::stage(id, "rollback", "start"));
            self.sup.start(id).map_err(|e| ArmoryError::UpdateFailed { reason: e.to_string(), rolled_back: false })?;
        }
        self.report(Progress::stage(id, "rollback", "done"));
        Ok(Updated { id: id.into(), from: cur.version, to: prev })
    }

    /// Delete version folders beyond `keep_versions`: the current and previous are always kept.
    pub(crate) fn prune_versions(&self, id: &str) {
        let (Ok(app_dir), Ok(Some(cur))) = (self.paths.app_dir(id), self.load_current(id)) else { return };
        let mut found: Vec<(Version, String)> = std::fs::read_dir(&app_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().to_string();
                Version::parse(&n).map(|v| (v, n))
            })
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0));
        let extra = self.settings.keep_versions.saturating_sub(2);
        let mut spare = extra;
        for (_, name) in found {
            if name == cur.version || cur.previous.as_deref() == Some(name.as_str()) {
                continue;
            }
            if spare > 0 {
                spare -= 1;
                continue;
            }
            let _ = remove_tree(&app_dir.join(&name));
        }
    }
}

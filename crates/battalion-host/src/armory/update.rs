// SPDX-License-Identifier: MPL-2.0
//! Update with rollback.
//!
//! ```text
//! (the old version keeps running while the new one downloads and is checked)
//! steps: download, verify, unpack, place, stop, switch, start, done      then commit
//! ```
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
    /// Update although the app reports work in flight.
    pub force: bool,
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
        let meta = self.load_meta(id).unwrap_or(Meta { channel: "stable".into(), state_schema: 1, previous_state_schema: None, custom_state_dir: false });
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
        if was_running && self.settings.check_busy && !opts.force {
            if let Ok(h) = self.sup.health(id) {
                if let Some(n) = h.busy.filter(|n| *n > 0) {
                    return Err(ArmoryError::AppBusy(n));
                }
            }
        }

        let (j, tx) = self.begin_tx(id, "update", &format!("{old}|{new}"))?;
        // Everything up to `place` leaves the running app alone.
        let staged = self.fetch_and_check(&entry.app, &feed, &plat, &key, &mut |s| self.step(&j, tx, s));
        let (dir, manifest) = match staged {
            Ok(x) => x,
            Err(e) => {
                let _ = j.abort(tx, &e.to_string());
                return Err(e);
            }
        };
        let new_schema = schema_of(&manifest);
        let vdir = self.paths.version_dir(id, &new)?;
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
    fn switch_and_start(&self, j: &crate::journal::Journal, tx: u64, id: &str, cur: &mut Current, cur_path: &Path, old: &str, new: &str, was_running: bool, meta: &Meta, new_schema: u32) -> Result<()> {
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
        let new_meta = Meta { state_schema: new_schema, previous_state_schema: Some(meta.state_schema), ..meta.clone() };
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
        if was_running {
            self.sup.stop(id).map_err(|e| ArmoryError::WrongState(e.to_string()))?;
        }
        let back = Current { version: prev.clone(), previous: Some(cur.version.clone()), state: AppState::Stopped, public_key: cur.public_key.clone() };
        store::write_json(&cur_path, &back)?;
        store::write_json(&meta_path(&self.paths, id)?, &Meta { previous_state_schema: Some(meta.state_schema), ..meta })?;
        if was_running {
            self.sup.start(id).map_err(|e| ArmoryError::UpdateFailed { reason: e.to_string(), rolled_back: false })?;
        }
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

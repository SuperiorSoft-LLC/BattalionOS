//! Journal recovery: run once when the OS starts, before apps are started or adopted.
//!
//! For each action that began and never ended, the last step it announced says how far it got:
//! - `install`: finished (app is `stopped` and its files are there) -> commit; anything else is
//!   undone to "absent".
//! - `update`: never switched -> the new files are dropped and the old version stays; switched
//!   but never confirmed (`done` missing) -> rolled back to the old version, unless the state
//!   schema changed, then the app is left `failed` for a person; confirmed -> commit.
//! - `remove`: rolled forward to the end.
//!
//! An app whose lock another Armory action holds is skipped and reported.

use super::*;
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovered {
    pub tx: u64,
    pub app: String,
    pub op: String,
    /// What was done: `committed`, `undone`, `rolled back`, `rolled forward`, `left failed`, `skipped (busy)`.
    pub outcome: String,
}

impl Armory {
    pub fn recover(&self) -> Result<Vec<Recovered>> {
        let j = crate::journal::Journal::open(self.paths.journal())?;
        let mut out = Vec::new();
        for p in j.pending()? {
            let done = |outcome: &str| Recovered { tx: p.tx, app: p.app.clone(), op: p.op.clone(), outcome: outcome.into() };
            let Ok(_lock) = self.lock_app(&p.app) else {
                out.push(done("skipped (busy)"));
                continue;
            };
            let outcome = match p.op.as_str() {
                "install" => self.recover_install(&p.app, &p.detail),
                "update" => self.recover_update(&p.app, &p.detail, p.steps.iter().any(|s| s == "done")),
                "remove" => self.recover_remove(&p.app, p.detail == "purge"),
                other => Err(ArmoryError::WrongState(format!("unknown journal action '{other}'"))),
            };
            match outcome {
                Ok(what) => {
                    if what == "committed" || what == "rolled forward" {
                        j.commit(p.tx)?;
                    } else {
                        j.abort(p.tx, &what)?;
                    }
                    out.push(done(&what));
                }
                Err(e) => {
                    j.abort(p.tx, &format!("recovery failed: {e}"))?;
                    out.push(done(&format!("failed: {e}")));
                }
            }
        }
        Ok(out)
    }

    fn recover_install(&self, id: &str, version: &str) -> Result<String> {
        let cur = self.load_current(id).unwrap_or(None);
        let finished = cur.as_ref().is_some_and(|c| c.version == version && c.state == AppState::Stopped) && self.paths.version_dir(id, version)?.join("app.json").is_file();
        if finished {
            return Ok("committed".into());
        }
        self.undo_install(id, version);
        Ok("undone".into())
    }

    fn recover_update(&self, id: &str, detail: &str, confirmed: bool) -> Result<String> {
        let (old, new) = detail.split_once('|').ok_or_else(|| ArmoryError::WrongState(format!("bad update record '{detail}'")))?;
        let cur_path = self.paths.current_json(id)?;
        let Some(mut cur) = self.load_current(id)? else { return Ok("undone".into()) };
        let _ = remove_tree(&self.paths.staging().join(format!("{id}-{new}.dir")));
        let _ = std::fs::remove_file(self.paths.staging().join(format!("{id}-{new}.zip.part")));
        if cur.version == new && confirmed {
            self.prune_versions(id);
            return Ok("committed".into());
        }
        if cur.version == old {
            // Never switched, or already rolled back. Drop the new files; settle a half state.
            if new != old {
                let _ = remove_tree(&self.paths.version_dir(id, new)?);
            }
            if matches!(cur.state, AppState::Updating) {
                cur.transition(&cur_path, AppState::Stopped)?;
            }
            return Ok("undone".into());
        }
        // Switched to `new`, never confirmed.
        let meta = self.load_meta(id);
        let schema_bumped = meta.as_ref().is_some_and(|m| m.previous_state_schema.is_some_and(|p| p != m.state_schema));
        if schema_bumped {
            let reason = format!("update to {new} was interrupted and the state schema changed; not rolled back");
            if cur.state.can_become(&AppState::Failed(String::new())) {
                cur.transition(&cur_path, AppState::Failed(reason))?;
            }
            return Ok("left failed".into());
        }
        if !self.paths.version_dir(id, old)?.join("app.json").is_file() {
            cur.transition(&cur_path, AppState::Failed(format!("update to {new} was interrupted and {old} is gone")))?;
            return Ok("left failed".into());
        }
        // A process of the new version may be up; stop it before its files go.
        let _ = self.sup.stop(id);
        let restored_meta = meta.map(|m| Meta { state_schema: m.previous_state_schema.unwrap_or(m.state_schema), previous_state_schema: None, ..m });
        let back = crate::state::Current { version: old.to_string(), previous: None, state: AppState::Stopped, public_key: cur.public_key.clone() };
        store::write_json(&cur_path, &back)?;
        if let Some(m) = restored_meta {
            store::write_json(&meta_path(&self.paths, id)?, &m)?;
        }
        let _ = remove_tree(&self.paths.version_dir(id, new)?);
        Ok("rolled back".into())
    }

    fn recover_remove(&self, id: &str, purge: bool) -> Result<String> {
        let cur_path = self.paths.current_json(id)?;
        if let Some(mut cur) = self.load_current(id)? {
            if cur.state != AppState::Removing && cur.state.can_become(&AppState::Removing) {
                cur.transition(&cur_path, AppState::Removing)?;
            }
        }
        self.delete_app_files(id, purge)?;
        let _ = std::fs::remove_file(&cur_path);
        Ok("rolled forward".into())
    }
}

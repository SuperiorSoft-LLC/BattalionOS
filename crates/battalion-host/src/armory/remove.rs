// SPDX-License-Identifier: MPL-2.0
//! Remove: the app's versions go, its state stays.
//!
//! The data folder (`data/<id>`) and web view folder are the user's: they are kept, so a later
//! install of the same app finds its state again. `purge_state` deletes the default folders on
//! request; a state folder the app named itself is never deleted by the OS.
//!
//! Steps: stop, mark, delete, forget (`current.json` last, so a crash leaves a `removing` app
//! that the next start finishes removing).

use super::*;
use crate::state::AppState;
use crate::supervisor::AppStatus;

#[derive(Debug, Clone, Default)]
pub struct RemoveOptions {
    pub purge_state: bool,
}

impl Armory {
    pub fn remove(&self, id: &str, opts: &RemoveOptions) -> Result<()> {
        self.paths.app_dir(id)?;
        let lock = self.lock_app(id)?;
        let cur = self.load_current(id)?.ok_or_else(|| ArmoryError::NotInstalled(id.into()))?;
        let (j, tx) = self.begin_tx(id, "remove", if opts.purge_state { "purge" } else { "keep-state" })?;
        let r = self.remove_steps(&j, tx, id, cur.state.clone(), opts.purge_state);
        match r {
            Ok(()) => {
                self.point("commit");
                j.commit(tx)?;
                drop(lock);
                if let Ok(d) = self.paths.app_dir(id) {
                    let _ = std::fs::remove_file(d.join("armory.lock"));
                    let _ = std::fs::remove_dir(d);
                }
                Ok(())
            }
            Err(e) => {
                let _ = j.abort(tx, &e.to_string());
                Err(e)
            }
        }
    }

    fn remove_steps(&self, j: &crate::journal::Journal, tx: u64, id: &str, state: AppState, purge: bool) -> Result<()> {
        let cur_path = self.paths.current_json(id)?;
        self.step(j, tx, "stop")?;
        if matches!(state, AppState::Starting | AppState::Running | AppState::Stopping) {
            let _: AppStatus = self.sup.stop(id).map_err(|e| ArmoryError::WrongState(format!("could not stop the app: {e}")))?;
        }
        let mut cur = self.load_current(id)?.ok_or_else(|| ArmoryError::NotInstalled(id.into()))?;
        if cur.state != AppState::Removing {
            if !cur.state.can_become(&AppState::Removing) {
                return Err(ArmoryError::WrongState(format!("app '{id}' is {}; it cannot be removed from there", cur.state.name())));
            }
            self.step(j, tx, "mark")?;
            cur.transition(&cur_path, AppState::Removing)?;
        }
        self.step(j, tx, "delete")?;
        self.delete_app_files(id, purge)?;
        self.step(j, tx, "forget")?;
        match std::fs::remove_file(&cur_path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Everything of the app except `current.json` (removed last) and, unless asked, its state.
    pub(crate) fn delete_app_files(&self, id: &str, purge: bool) -> Result<()> {
        let app_dir = self.paths.app_dir(id)?;
        let custom = self.load_meta(id).is_some_and(|m| m.custom_state_dir);
        if let Ok(rd) = std::fs::read_dir(&app_dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    remove_tree(&p)?; // version folders
                }
            }
        }
        let _ = std::fs::remove_file(meta_path(&self.paths, id)?);
        let _ = remove_tree(&self.paths.root().join("run").join(id));
        if purge && !custom {
            remove_tree(&self.paths.data_dir(id)?)?;
            remove_tree(&self.paths.webview_dir(id)?)?;
        }
        Ok(())
    }
}

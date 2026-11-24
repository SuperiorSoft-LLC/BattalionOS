//! One state machine per app, persisted in `current.json` before each step, so a crash of the
//! OS resumes or undoes the step.
//!
//! ```text
//! absent -> downloading -> verifying -> installed -> initialising -> stopped <-> starting -> running -> stopping -> stopped
//! (installed | stopped | running) -> updating -> stopped | running(after rollback)
//! any -> failed(reason) -> (downloading | stopped | removing)      (Retry, or give up)
//! (installed | stopped | failed) -> removing -> absent
//! ```

use crate::store;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "reason", rename_all = "snake_case")]
pub enum AppState {
    Absent,
    Downloading,
    Verifying,
    Installed,
    Initialising,
    Stopped,
    Starting,
    Running,
    Stopping,
    Updating,
    Failed(String),
    Removing,
}

impl AppState {
    pub fn name(&self) -> &'static str {
        match self {
            AppState::Absent => "absent",
            AppState::Downloading => "downloading",
            AppState::Verifying => "verifying",
            AppState::Installed => "installed",
            AppState::Initialising => "initialising",
            AppState::Stopped => "stopped",
            AppState::Starting => "starting",
            AppState::Running => "running",
            AppState::Stopping => "stopping",
            AppState::Updating => "updating",
            AppState::Failed(_) => "failed",
            AppState::Removing => "removing",
        }
    }

    /// Whether `self -> to` is a legal step. Anything may fail; nothing may skip verification.
    pub fn can_become(&self, to: &AppState) -> bool {
        use AppState::*;
        if matches!(to, Failed(_)) {
            return !matches!(self, Absent);
        }
        match (self, to) {
            (Absent, Downloading)
            | (Downloading, Verifying)
            | (Verifying, Installed)
            | (Installed, Initialising)
            | (Installed, Stopped) // an app without an init part
            | (Initialising, Stopped)
            | (Stopped, Starting)
            | (Starting, Running)
            | (Starting, Stopped) // never ready: stopped again, with the reason on `failed` first
            | (Running, Stopping)
            | (Stopping, Stopped)
            | (Stopped, Updating)
            | (Running, Updating)
            | (Installed, Updating)
            | (Updating, Stopped)
            | (Updating, Running) // rollback to the previous version, started again
            | (Stopped, Removing)
            | (Installed, Removing)
            | (Removing, Absent)
            | (Failed(_), Downloading) // Retry
            | (Failed(_), Stopped)
            | (Failed(_), Removing) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IllegalTransition {
    pub from: String,
    pub to: String,
}
impl std::fmt::Display for IllegalTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "an app cannot go from {} to {}", self.from, self.to)
    }
}
impl std::error::Error for IllegalTransition {}

/// `apps/<id>/current.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Current {
    pub version: String,
    #[serde(default)]
    pub previous: Option<String>,
    #[serde(flatten)]
    pub state: AppState,
    /// minisign key pinned at first install.
    #[serde(default, rename = "publicKey")]
    pub public_key: String,
}

impl Current {
    pub fn load(path: &Path) -> io::Result<Option<Current>> {
        store::read_json(path)
    }

    /// Move to `to` and persist, or refuse without touching the file.
    pub fn transition(&mut self, path: &Path, to: AppState) -> io::Result<()> {
        if !self.state.can_become(&to) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                IllegalTransition { from: self.state.name().into(), to: to.name().into() },
            ));
        }
        let mut next = self.clone();
        next.state = to;
        store::write_json(path, &next)?;
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn happy_path_and_refusals() {
        use AppState::*;
        let path = [Absent, Downloading, Verifying, Installed, Initialising, Stopped, Starting, Running, Stopping, Stopped, Removing, Absent];
        for w in path.windows(2) {
            assert!(w[0].can_become(&w[1]), "{:?} -> {:?}", w[0], w[1]);
        }
        assert!(!Absent.can_become(&Running));
        assert!(!Downloading.can_become(&Installed), "verification cannot be skipped");
        assert!(!Running.can_become(&Removing), "stop before remove");
        assert!(!Absent.can_become(&Failed("x".into())));
        assert!(Running.can_become(&Failed("x".into())));
    }
}

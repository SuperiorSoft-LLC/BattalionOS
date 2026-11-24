// SPDX-License-Identifier: MPL-2.0
//! Progress of an Armory action, pushed to whoever listens (the desktop turns it into an event).
//!
//! One report is `{id, op, stage}` and, while a package downloads, also `bytes`, `total` and
//! `percent`. Stages are the journal's step names (`download`, `verify`, `unpack`, `place`, `init`,
//! `stop`, `switch`, `start`, `done`, `mark`, `delete`, `forget`). The sink is called on the
//! thread that runs the action and must not block.

use super::*;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub id: String,
    /// `install`, `update`, `remove` or `rollback`.
    pub op: String,
    pub stage: String,
    pub bytes: Option<u64>,
    pub total: Option<u64>,
    /// 0..=100 while the size is known; `None` for a stage without a measure.
    pub percent: Option<u8>,
}

impl Progress {
    pub fn stage(id: &str, op: &str, stage: &str) -> Progress {
        Progress { id: id.into(), op: op.into(), stage: stage.into(), bytes: None, total: None, percent: None }
    }
    pub fn bytes(id: &str, op: &str, bytes: u64, total: u64) -> Progress {
        let percent = if total == 0 { 100 } else { (bytes.min(total) * 100 / total) as u8 };
        Progress { id: id.into(), op: op.into(), stage: "download".into(), bytes: Some(bytes), total: Some(total), percent: Some(percent) }
    }
}

pub type ProgressSink = Arc<dyn Fn(&Progress) + Send + Sync>;

/// Limits the download reports to one per percent step and per `Settings`-free 100 ms, and always
/// lets the last one (all bytes) through.
pub(crate) struct Pace {
    last_pct: i64,
    last_at: Option<Instant>,
}
impl Pace {
    pub fn new() -> Pace {
        Pace { last_pct: -1, last_at: None }
    }
    pub fn tick(&mut self, got: u64, total: u64) -> Option<(u64, u64)> {
        let pct = if total == 0 { 100 } else { (got.min(total) * 100 / total) as i64 };
        let done = got >= total;
        let due = self.last_at.is_none_or(|t| t.elapsed() >= Duration::from_millis(100));
        if pct != self.last_pct && (due || done) {
            self.last_pct = pct;
            self.last_at = Some(Instant::now());
            return Some((got, total));
        }
        None
    }
}

impl Armory {
    /// Who hears the progress of every action. One sink at a time; `None` stops the reports.
    pub fn set_progress_sink(&self, sink: Option<ProgressSink>) {
        *self.sink.lock().unwrap_or_else(|p| p.into_inner()) = sink;
    }

    pub(crate) fn report(&self, p: Progress) {
        let sink = self.sink.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(s) = sink {
            s(&p);
        }
    }
}

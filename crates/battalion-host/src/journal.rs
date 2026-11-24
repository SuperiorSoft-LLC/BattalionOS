//! Write-ahead journal of multi-step actions (install, update, remove). Written before each step,
//! so after a crash the next start knows what was in flight and can finish or undo it.
//!
//! One JSON object per line, appended and synced. A torn last line (crash mid-write) is ignored.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JournalEntry {
    /// An action starts. `op` is free text of the caller (`install`, `update`, `remove`).
    Begin { tx: u64, app: String, op: String, #[serde(default)] detail: String },
    /// A step is about to run. Written before it, so a crash inside it is visible.
    Step { tx: u64, step: String },
    /// Finished, all steps done.
    Commit { tx: u64 },
    /// Finished by undoing.
    Abort { tx: u64, #[serde(default)] reason: String },
}

/// An action that began and never committed or aborted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub tx: u64,
    pub app: String,
    pub op: String,
    pub detail: String,
    /// Steps announced, in order. The last one is where it died (it may or may not have completed).
    pub steps: Vec<String>,
}

pub struct Journal {
    path: PathBuf,
    next_tx: u64,
}

impl Journal {
    pub fn open(path: impl Into<PathBuf>) -> io::Result<Journal> {
        let path = path.into();
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let next_tx = read_entries(&path)?
            .iter()
            .map(|e| match e {
                JournalEntry::Begin { tx, .. } | JournalEntry::Step { tx, .. } | JournalEntry::Commit { tx } | JournalEntry::Abort { tx, .. } => *tx,
            })
            .max()
            .map_or(1, |m| m + 1);
        Ok(Journal { path, next_tx })
    }

    fn append(&self, e: &JournalEntry) -> io::Result<()> {
        let mut line = serde_json::to_vec(e).map_err(io::Error::other)?;
        line.push(b'\n');
        let mut f = OpenOptions::new().create(true).append(true).open(&self.path)?;
        f.write_all(&line)?;
        f.sync_all()
    }

    pub fn begin(&mut self, app: &str, op: &str, detail: &str) -> io::Result<u64> {
        let tx = self.next_tx;
        self.append(&JournalEntry::Begin { tx, app: app.into(), op: op.into(), detail: detail.into() })?;
        self.next_tx += 1;
        Ok(tx)
    }
    pub fn step(&self, tx: u64, step: &str) -> io::Result<()> {
        self.append(&JournalEntry::Step { tx, step: step.into() })
    }
    pub fn commit(&self, tx: u64) -> io::Result<()> {
        self.append(&JournalEntry::Commit { tx })
    }
    pub fn abort(&self, tx: u64, reason: &str) -> io::Result<()> {
        self.append(&JournalEntry::Abort { tx, reason: reason.into() })
    }

    /// Actions in flight, oldest first. Recovery: for each, finish or undo it, then commit or abort it.
    pub fn pending(&self) -> io::Result<Vec<Pending>> {
        let mut open: BTreeMap<u64, Pending> = BTreeMap::new();
        for e in read_entries(&self.path)? {
            match e {
                JournalEntry::Begin { tx, app, op, detail } => {
                    open.insert(tx, Pending { tx, app, op, detail, steps: Vec::new() });
                }
                JournalEntry::Step { tx, step } => {
                    if let Some(p) = open.get_mut(&tx) {
                        p.steps.push(step);
                    }
                }
                JournalEntry::Commit { tx } | JournalEntry::Abort { tx, .. } => {
                    open.remove(&tx);
                }
            }
        }
        Ok(open.into_values().collect())
    }
}

fn read_entries(path: &Path) -> io::Result<Vec<JournalEntry>> {
    let text = match std::fs::read(path) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    // A line that does not parse is a torn write; skipping it is the recovery.
    Ok(text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
}

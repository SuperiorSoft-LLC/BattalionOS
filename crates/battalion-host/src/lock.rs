//! Exclusive file locks: one per app (two Armory actions on one app) and one for the whole OS
//! (single instance). The OS releases a lock when the process dies, so a crash never leaves one stuck.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

#[derive(Debug)]
pub enum LockError {
    /// Another holder has it. Not an I/O failure: callers say "busy" and retry later.
    Busy,
    Io(io::Error),
}
impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockError::Busy => write!(f, "locked by another holder"),
            LockError::Io(e) => write!(f, "lock failed: {e}"),
        }
    }
}
impl std::error::Error for LockError {}

/// Held until dropped.
#[derive(Debug)]
pub struct Lock {
    file: File,
}

impl Lock {
    pub fn try_acquire(path: &Path) -> Result<Lock, LockError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(LockError::Io)?;
        }
        let file = OpenOptions::new().create(true).truncate(false).write(true).open(path).map_err(LockError::Io)?;
        match file.try_lock() {
            Ok(()) => Ok(Lock { file }),
            Err(TryLockError::WouldBlock) => Err(LockError::Busy),
            Err(TryLockError::Error(e)) => Err(LockError::Io(e)),
        }
    }

    /// Wait up to `timeout`, polling; for short critical sections.
    pub fn acquire_within(path: &Path, timeout: std::time::Duration) -> Result<Lock, LockError> {
        let start = std::time::Instant::now();
        loop {
            match Lock::try_acquire(path) {
                Err(LockError::Busy) if start.elapsed() < timeout => std::thread::sleep(std::time::Duration::from_millis(10)),
                other => return other,
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

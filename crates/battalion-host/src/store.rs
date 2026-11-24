// SPDX-License-Identifier: MPL-2.0
//! Atomic file writes: a reader sees the old file or the new one, never half of one.
//! Write to a temp file in the same folder, flush it to disk, then rename over the target.

use serde::{de::DeserializeOwned, Serialize};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_sibling(path: &Path) -> io::Result<std::path::PathBuf> {
    let name = path.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    // pid + counter: two writers in one or two processes never share a temp file.
    let tmp = format!(".{}.{}.{}.tmp", name.to_string_lossy(), std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed));
    Ok(path.with_file_name(tmp))
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = temp_sibling(path)?;
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    bytes.push(b'\n');
    write_atomic(path, &bytes)
}

/// `Ok(None)` when the file does not exist; an unreadable or invalid file is an error, never
/// silently "absent" (a corrupt `current.json` must not look like an app that was never installed).
pub fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match std::fs::read(path) {
        Ok(b) => serde_json::from_slice(&b)
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{}: {e}", path.display()))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Remove temp files a crashed writer left in `dir`. Returns how many.
pub fn sweep_temp(dir: &Path) -> io::Result<usize> {
    let mut n = 0;
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') && name.ends_with(".tmp") && std::fs::remove_file(e.path()).is_ok() {
            n += 1;
        }
    }
    Ok(n)
}

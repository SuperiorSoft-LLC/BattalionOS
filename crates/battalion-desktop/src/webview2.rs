// SPDX-License-Identifier: MPL-2.0
//! A stale WebView2 registration must not leave the window blank.
//!
//! Windows finds the WebView2 runtime through the registry: `...\EdgeUpdate\Clients\{F3017226-...}` names a version
//! (`pv`) and a folder (`location`). Some machines (Windows Sandbox's image is one) keep a `pv` whose version folder is
//! gone while a newer one sits next to it. Then the loader says "Could not find the WebView2 Runtime" and the window
//! stays white. The loader honours `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` first, so when the registered folder is missing
//! and a version folder with the runtime exists, the OS points that variable at the newest one before any window opens.
//! A machine whose registration is fine is left alone (the evergreen lookup stays in charge).

use std::path::{Path, PathBuf};

pub const FOLDER_VAR: &str = "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER";
const RUNTIME_EXE: &str = "msedgewebview2.exe";

fn version_key(name: &str) -> Option<Vec<u32>> {
    let parts: Option<Vec<u32>> = name.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| p.len() >= 2)
}

fn has_runtime(dir: &Path) -> bool {
    dir.join(RUNTIME_EXE).is_file()
}

/// The newest version folder under `application_dir` that holds the runtime executable.
pub fn newest_runtime(application_dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(Vec<u32>, PathBuf)> = None;
    for e in std::fs::read_dir(application_dir).into_iter().flatten().flatten() {
        let path = e.path();
        let Some(key) = e.file_name().to_str().and_then(version_key) else { continue };
        if has_runtime(&path) && best.as_ref().is_none_or(|(k, _)| key > *k) {
            best = Some((key, path));
        }
    }
    best.map(|(_, p)| p)
}

/// The folder to force, or None when the registration works (or no runtime exists to force).
/// `registered` is the registry's (`location`, `pv`).
pub fn fallback_folder(registered: Option<(&Path, &str)>) -> Option<PathBuf> {
    let (location, pv) = registered?;
    if has_runtime(&location.join(pv)) {
        return None;
    }
    newest_runtime(location)
}

/// Parse `reg query` output: the data of value `name` (`    pv    REG_SZ    157.0.1`).
pub fn reg_value(output: &str, name: &str) -> Option<String> {
    output.lines().find_map(|l| {
        let rest = l.trim().strip_prefix(name)?;
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        let data = rest.trim_start().strip_prefix("REG_SZ")?.trim();
        (!data.is_empty()).then(|| data.to_string())
    })
}

#[cfg(windows)]
fn registered() -> Option<(PathBuf, String)> {
    use std::os::windows::process::CommandExt;
    const CLIENT: &str = r"EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    for root in [r"HKLM\SOFTWARE\WOW6432Node\Microsoft", r"HKLM\SOFTWARE\Microsoft", r"HKCU\SOFTWARE\Microsoft"] {
        let out = std::process::Command::new("reg")
            .args(["query", &format!(r"{root}\{CLIENT}")])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        if let (Some(pv), Some(loc)) = (reg_value(&text, "pv"), reg_value(&text, "location")) {
            return Some((PathBuf::from(loc), pv));
        }
    }
    None
}

/// Call once at start, before the first window. Returns the folder it forced, if any.
pub fn apply_fallback() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if std::env::var_os(FOLDER_VAR).is_some() {
            return None;
        }
        let (loc, pv) = registered()?;
        let dir = fallback_folder(Some((&loc, &pv)))?;
        // SAFETY: called from main before any other thread starts.
        unsafe { std::env::set_var(FOLDER_VAR, &dir) };
        return Some(dir);
    }
    #[allow(unreachable_code)]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime(root: &Path, ver: &str) {
        std::fs::create_dir_all(root.join(ver)).unwrap();
        std::fs::write(root.join(ver).join(RUNTIME_EXE), b"x").unwrap();
    }

    #[test]
    fn picks_the_newest_version_by_number_not_by_text() {
        let d = tempfile::tempdir().unwrap();
        runtime(d.path(), "99.0.1.1");
        runtime(d.path(), "158.0.4370.33");
        runtime(d.path(), "157.0.4341.60");
        std::fs::create_dir_all(d.path().join("SetupMetrics")).unwrap();
        std::fs::create_dir_all(d.path().join("159.0.1.1")).unwrap(); // no runtime inside: ignored
        assert_eq!(newest_runtime(d.path()).unwrap(), d.path().join("158.0.4370.33"));
    }

    #[test]
    fn a_stale_registration_falls_back_and_a_good_one_is_left_alone() {
        let d = tempfile::tempdir().unwrap();
        runtime(d.path(), "157.0.4341.60");
        runtime(d.path(), "158.0.4370.33");
        // The Sandbox case: pv names a folder that is gone.
        assert_eq!(fallback_folder(Some((d.path(), "157.0.4341.46"))), Some(d.path().join("158.0.4370.33")));
        // Registered folder exists: nothing to force.
        assert_eq!(fallback_folder(Some((d.path(), "157.0.4341.60"))), None);
        // No registration, or nothing installed: nothing to force.
        assert_eq!(fallback_folder(None), None);
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(fallback_folder(Some((empty.path(), "1.2.3.4"))), None);
    }

    #[test]
    fn reads_reg_query_output() {
        let out = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\X\r\n    name    REG_SZ    Microsoft EdgeWebView\r\n    pv    REG_SZ    157.0.4341.46\r\n    location    REG_SZ    C:\\Program Files (x86)\\Microsoft\\EdgeWebView\\Application\r\n";
        assert_eq!(reg_value(out, "pv").as_deref(), Some("157.0.4341.46"));
        assert_eq!(reg_value(out, "location").as_deref(), Some(r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application"));
        assert_eq!(reg_value(out, "missing"), None);
    }
}

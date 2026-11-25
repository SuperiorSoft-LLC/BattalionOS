// SPDX-License-Identifier: MPL-2.0
//! One Start menu shortcut per installed app (BOS-19), written by the OS: after an install, update, rollback or remove, and
//! once at every start (so a shortcut lost or left over is put right). The shortcut runs `<this program> --open <id>`, has
//! the app's name and its icon (an `.ico` the app ships; otherwise the OS's own). Windows only; elsewhere it does nothing
//! (the platform's own menu entry is a later piece). The folder is `Start Menu\Programs\BattalionOS Apps`.

use crate::launch::{plan, Want};
use battalion_host::armory::Armory;
use battalion_host::Paths;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MAP: &str = "shortcuts.json";

/// The folder the shortcuts live in (Windows: the user's Start menu programs).
pub fn menu_dir() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("Microsoft").join("Windows").join("Start Menu").join("Programs").join("BattalionOS Apps"))
}

/// What the OS wants now: every installed app with its name and icon file.
pub fn wanted(paths: &Paths, armory: &Armory) -> Vec<Want> {
    let Ok(list) = armory.installed() else { return Vec::new() };
    list.into_iter()
        .filter(|a| a.state != "removing")
        .map(|a| {
            let icon = paths.content_dir(&a.id, &a.version).ok().and_then(|v| ico_of(&v));
            Want { name: a.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| a.id.clone()), id: a.id, icon }
        })
        .collect()
}

/// The app's `.ico` named by its manifest `icon` (a relative path kept under the version folder), if it ships one.
fn ico_of(vdir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(vdir.join("app.json")).ok()?;
    let rel = serde_json::from_str::<serde_json::Value>(&text).ok()?.get("icon")?.as_str()?.to_string();
    if !rel.to_ascii_lowercase().ends_with(".ico") || !battalion_app_api::manifest::safe_rel_path(&rel) {
        return None;
    }
    let f = vdir.join(&rel).canonicalize().ok()?;
    f.starts_with(vdir.canonicalize().ok()?).then_some(f)
}

/// Make the Start menu match the installed apps. Returns what went wrong, if anything (never panics, never blocks an install).
pub fn sync(paths: &Paths, armory: &Armory) -> Result<(), String> {
    let Some(dir) = menu_dir() else { return Ok(()) };
    let map_path = paths.root().join(MAP);
    let have: BTreeMap<String, String> = std::fs::read_to_string(&map_path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let want = wanted(paths, armory);
    let (write, del) = plan(&want, &have);
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    apply(&dir, &exe, &want, &write, &del)?;
    let new: BTreeMap<String, String> = write.into_iter().collect();
    std::fs::write(&map_path, serde_json::to_string_pretty(&new).unwrap_or_default()).map_err(|e| e.to_string())?;
    if new.is_empty() {
        let _ = std::fs::remove_dir(&dir); // only when empty
    }
    Ok(())
}

#[cfg(windows)]
fn apply(dir: &Path, exe: &Path, want: &[Want], write: &[(String, String)], del: &[String]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    for f in del {
        let _ = std::fs::remove_file(dir.join(f));
    }
    if write.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let q = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let mut ps = String::from("$ErrorActionPreference='Stop';$sh=New-Object -ComObject WScript.Shell;");
    for (id, file) in write {
        let icon = want.iter().find(|w| &w.id == id).and_then(|w| w.icon.clone()).unwrap_or_else(|| exe.to_path_buf());
        let lnk = dir.join(file);
        ps.push_str(&format!(
            "$s=$sh.CreateShortcut({});$s.TargetPath={};$s.Arguments={};$s.WorkingDirectory={};$s.IconLocation={};$s.Save();",
            q(&lnk.to_string_lossy()),
            q(&exe.to_string_lossy()),
            q(&format!("--open {id}")),
            q(&exe.parent().unwrap_or(Path::new(".")).to_string_lossy()),
            q(&format!("{},0", icon.to_string_lossy())),
        ));
    }
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(not(windows))]
fn apply(_: &Path, _: &Path, _: &[Want], _: &[(String, String)], _: &[String]) -> Result<(), String> {
    Ok(())
}

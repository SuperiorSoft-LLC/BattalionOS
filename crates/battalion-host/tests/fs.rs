// SPDX-License-Identifier: MPL-2.0
//! FS-API-1: the OS root and the read side. Every escape form is refused; links and junctions out are refused at the open.

use battalion_host::fs::{check_rel, Fs, FsError, Settings, Watches};
use battalion_host::Paths;
use serde_json::json;
use std::path::Path;

fn fs_in(dir: &Path) -> Fs {
    Fs::with_root(&dir.join("root"), None, Settings::default()).unwrap()
}

/// A link `link` -> `target` (a junction on Windows, which needs no admin; a symlink elsewhere). False if not possible.
fn dir_link(link: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}

fn file_link(link: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link).is_ok()
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}

#[test]
fn dot_dot_in_every_position_is_refused() {
    for p in ["..", "../x", "a/..", "a/../b", "a/b/..", "../../..", "a/./b", ".", "./a", "a/.", "..\\x", "a/..\\..", "..."] {
        assert!(matches!(check_rel(p, 1024), Err(FsError::BadPath(_))), "{p}");
    }
}

#[test]
fn absolute_drive_device_and_odd_forms_are_refused() {
    for p in [
        "/etc/passwd", "\\Windows", "C:\\Windows", "C:/Windows", "c:x", "\\\\server\\share", "//server/share", "a//b", "a/", "a\\b",
        "file.txt:stream", "CON", "a/nul", "a/COM1.txt", "LPT3", "a/aux.log", "x\0y", "a\nb", "trail.", "trail ", "a/b?", "a*b", "a|b", "<a>",
    ] {
        assert!(matches!(check_rel(p, 1024), Err(FsError::BadPath(_))), "{p:?}");
    }
    assert_eq!(check_rel("", 1024).unwrap(), Vec::<&str>::new());
    assert_eq!(check_rel("docs/a b/ok.txt", 1024).unwrap(), vec!["docs", "a b", "ok.txt"]);
}

#[test]
fn long_paths_up_to_the_setting_work_and_longer_are_refused() {
    let t = tempfile::tempdir().unwrap();
    let fs = fs_in(t.path());
    // Deeper than Windows' old 260 characters: the root's real path is a verbatim one, so it works.
    let segs: Vec<String> = (0..30).map(|i| format!("folder-number-{i:02}")).collect();
    let rel = segs.join("/");
    assert!(rel.len() > 260);
    std::fs::create_dir_all(fs.root().join(segs.iter().collect::<std::path::PathBuf>())).unwrap();
    std::fs::write(fs.resolve(&format!("{rel}/deep.txt")).unwrap(), "deep").unwrap();
    assert_eq!(fs.read(&format!("{rel}/deep.txt"), 0, None, true).unwrap()["data"], "deep");
    let over = "a/".repeat(600) + "a";
    assert!(matches!(fs.stat(&over), Err(FsError::BadPath(m)) if m.contains("fs.maxPathChars")));
}

#[test]
fn list_stat_read_inside_the_root() {
    let t = tempfile::tempdir().unwrap();
    let fs = fs_in(t.path());
    std::fs::create_dir_all(fs.root().join("docs/sub")).unwrap();
    std::fs::write(fs.root().join("docs/a.txt"), "héllo").unwrap();
    std::fs::write(fs.root().join("docs/b.bin"), [0xffu8, 0, 1]).unwrap();
    let l = fs.list("docs", None, None).unwrap();
    let names: Vec<&str> = l["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["a.txt", "b.bin", "sub"]);
    assert_eq!(l["entries"][2]["kind"], "dir");
    assert_eq!(l["cursor"], serde_json::Value::Null);
    // Paged.
    let p1 = fs.list("docs", None, Some(2)).unwrap();
    assert_eq!(p1["entries"].as_array().unwrap().len(), 2);
    let p2 = fs.list("docs", p1["cursor"].as_str(), Some(2)).unwrap();
    assert_eq!(p2["entries"][0]["name"], "sub");
    // The root itself.
    assert_eq!(fs.list("", None, None).unwrap()["entries"][0]["name"], "docs");
    let s = fs.stat("docs/a.txt").unwrap();
    assert_eq!((s["kind"].as_str(), s["size"].as_u64(), s["name"].as_str()), (Some("file"), Some(6), Some("a.txt")));
    assert!(!s["display"].as_str().unwrap().starts_with(r"\\?\"));
    assert_eq!(fs.read("docs/a.txt", 0, None, true).unwrap()["data"], "héllo");
    assert!(matches!(fs.read("docs/b.bin", 0, None, true), Err(FsError::NotText)));
    assert_eq!(fs.read("docs/b.bin", 0, None, false).unwrap()["data"], "/wAB");
    assert!(matches!(fs.read("docs", 0, None, true), Err(FsError::NotAFile)));
    assert!(matches!(fs.list("docs/a.txt", None, None), Err(FsError::NotAFolder)));
    assert!(matches!(fs.stat("docs/none"), Err(FsError::NotFound)));
}

#[test]
fn read_is_capped_by_the_setting_and_chunks_on_a_character_boundary() {
    let t = tempfile::tempdir().unwrap();
    let fs = Fs::with_root(&t.path().join("root"), None, Settings { read_max_bytes: 4, ..Settings::default() }).unwrap();
    std::fs::write(fs.root().join("t.txt"), "abcé€x").unwrap(); // 1+1+1+2+3+1 = 9 bytes
    let r = fs.read("t.txt", 0, Some(100), true).unwrap();
    assert_eq!((r["data"].as_str(), r["eof"].as_bool(), r["length"].as_u64()), (Some("abc"), Some(false), Some(3)));
    let r = fs.read("t.txt", 3, None, true).unwrap();
    assert_eq!(r["data"], "é");
    let r = fs.read("t.txt", 5, None, true).unwrap();
    assert_eq!((r["data"].as_str(), r["eof"].as_bool()), (Some("€x"), Some(true)));
    assert_eq!(fs.read("t.txt", 0, Some(2), false).unwrap()["length"], 2);
}

#[test]
fn a_junction_or_symlink_pointing_out_is_refused_one_inside_is_followed() {
    let t = tempfile::tempdir().unwrap();
    let fs = fs_in(t.path());
    let outside = t.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "no").unwrap();
    std::fs::create_dir_all(fs.root().join("inner")).unwrap();
    std::fs::write(fs.root().join("inner/ok.txt"), "yes").unwrap();
    if !dir_link(&fs.root().join("out"), &outside) || !dir_link(&fs.root().join("in"), &fs.root().join("inner")) {
        eprintln!("skipped: cannot make a directory link here");
        return;
    }
    assert_eq!(fs.list("out", None, None), Err(FsError::LeavesRoot));
    assert_eq!(fs.stat("out/secret.txt"), Err(FsError::LeavesRoot));
    assert_eq!(fs.read("out/secret.txt", 0, None, true), Err(FsError::LeavesRoot));
    assert!(Watches::default().add(&fs, "aa", "out").is_err());
    // Listed (as a link), but not followed.
    let l = fs.list("", None, None).unwrap();
    assert!(l["entries"].as_array().unwrap().iter().any(|e| e["name"] == "out" && e["kind"] == "link"));
    // A link inside the root is followed.
    assert_eq!(fs.read("in/ok.txt", 0, None, true).unwrap()["data"], "yes");
    assert_eq!(fs.stat("in").unwrap()["kind"], "link");
}

#[test]
fn a_file_symlink_pointing_out_is_refused() {
    let t = tempfile::tempdir().unwrap();
    let fs = fs_in(t.path());
    std::fs::write(t.path().join("secret.txt"), "no").unwrap();
    if !file_link(&fs.root().join("s.txt"), &t.path().join("secret.txt")) {
        eprintln!("skipped: no symlink right here (Windows without developer mode)");
        return;
    }
    assert_eq!(fs.read("s.txt", 0, None, true), Err(FsError::LeavesRoot));
}

#[test]
fn a_folder_swapped_for_a_junction_after_a_check_is_refused_at_the_open() {
    let t = tempfile::tempdir().unwrap();
    let fs = fs_in(t.path());
    let outside = t.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("x.txt"), "no").unwrap();
    std::fs::create_dir_all(fs.root().join("d")).unwrap();
    std::fs::write(fs.root().join("d/x.txt"), "yes").unwrap();
    // The lexical check passes and the first read is fine.
    assert!(fs.resolve("d/x.txt").is_ok());
    assert_eq!(fs.read("d/x.txt", 0, None, true).unwrap()["data"], "yes");
    let mut ws = Watches::default();
    ws.add(&fs, "aa", "d").unwrap();
    // Swap the folder for a link out.
    std::fs::remove_dir_all(fs.root().join("d")).unwrap();
    if !dir_link(&fs.root().join("d"), &outside) {
        eprintln!("skipped: cannot make a directory link here");
        return;
    }
    assert_eq!(fs.read("d/x.txt", 0, None, true), Err(FsError::LeavesRoot));
    // A watch on it reports the path gone, never the outside folder's entries.
    let changes = ws.poll(&fs);
    assert!(changes.iter().all(|(_, _, c)| c["change"] == "removed"), "{changes:?}");
}

#[test]
fn the_os_home_under_the_root_stays_out_of_reach() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("root/oshome");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("os.json"), "{}").unwrap();
    let fs = Fs::with_root(&t.path().join("root"), Some(&home), Settings::default()).unwrap();
    assert_eq!(fs.stat("oshome/os.json"), Err(FsError::LeavesRoot));
    assert_eq!(fs.list("oshome", None, None), Err(FsError::LeavesRoot));
    assert!(fs.list("", None, None).unwrap()["entries"].as_array().unwrap().is_empty());
}

#[test]
fn the_root_comes_from_os_json_and_is_created_when_missing() {
    let t = tempfile::tempdir().unwrap();
    let paths = Paths::new(t.path().join("home"));
    std::fs::create_dir_all(paths.root()).unwrap();
    let want = t.path().join("my files");
    std::fs::write(paths.os_json(), serde_json::to_string(&json!({"fs": {"root": want, "listMax": 7}})).unwrap()).unwrap();
    let fs = Fs::open(&paths).unwrap();
    assert!(want.is_dir());
    assert_eq!(fs.settings.list_max, 7);
    assert_eq!(fs.root(), std::fs::canonicalize(&want).unwrap());
    // Default: <user home>/BattalionOS.
    assert!(Settings::default().root_path().unwrap().ends_with("BattalionOS"));
}

#[test]
fn watches_report_changes_and_are_capped_per_app() {
    let t = tempfile::tempdir().unwrap();
    let fs = Fs::with_root(&t.path().join("root"), None, Settings { watch_max: 2, ..Settings::default() }).unwrap();
    std::fs::create_dir_all(fs.root().join("w")).unwrap();
    let mut ws = Watches::default();
    let id = ws.add(&fs, "aa", "w").unwrap();
    ws.add(&fs, "aa", "").unwrap();
    assert_eq!(ws.add(&fs, "aa", "w"), Err(FsError::TooManyWatches(2)));
    assert!(ws.add(&fs, "bb", "w").is_ok(), "the cap is per app");
    assert!(ws.poll(&fs).is_empty());
    std::fs::write(fs.root().join("w/new.txt"), "1").unwrap();
    let c = ws.poll(&fs);
    assert!(c.iter().any(|(app, w, c)| app == "aa" && *w == id && c["path"] == "w/new.txt" && c["change"] == "created"), "{c:?}");
    assert_eq!(ws.remove("bb", &id), Err(FsError::NoSuchWatch), "another app's watch id");
    ws.remove("aa", &id).unwrap();
    assert!(matches!(ws.add(&fs, "aa", "../x"), Err(FsError::BadPath(_))));
}

#[test]
fn page_call_args_are_checked() {
    let t = tempfile::tempdir().unwrap();
    let fs = fs_in(t.path());
    std::fs::write(fs.root().join("a.txt"), "hi").unwrap();
    assert_eq!(fs.call("os_fs_read", &json!({"path": "a.txt", "as": "base64"})).unwrap()["data"], "aGk=");
    assert!(matches!(fs.call("os_fs_read", &json!({"path": "a.txt", "as": "hex"})), Err(FsError::BadArgs(_))));
    assert!(matches!(fs.call("os_fs_list", &json!({"path": 3})), Err(FsError::BadArgs(_))));
    assert!(matches!(fs.call("os_fs_write", &json!({"path": "a.txt"})), Err(FsError::BadArgs(_))));
    assert_eq!(fs.call("os_fs_stat", &json!({})).unwrap()["kind"], "dir");
}

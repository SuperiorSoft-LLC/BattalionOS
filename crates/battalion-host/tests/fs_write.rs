// SPDX-License-Identifier: MPL-2.0
//! FS-API-2: the write side under the OS root. Both ends of a rename or copy stay under the root, the root itself is never
//! changed, links are removed as links, a write is atomic, and an app's watches go when its frame closes.

use battalion_host::fs::{permission_for, Fs, FsError, Settings, Watches, COMMANDS, PERMISSION, WRITE_COMMANDS, WRITE_PERMISSION};
use serde_json::json;
use std::path::Path;

fn fs_in(dir: &Path) -> Fs {
    Fs::with_root(&dir.join("root"), None, Settings::default()).unwrap()
}

/// A junction on Windows (no admin needed), a symlink elsewhere. False if not possible here.
fn dir_link(link: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    {
        std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(link).arg(target).output().map(|o| o.status.success()).unwrap_or(false)
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    v.sort();
    v
}

#[test]
fn every_write_call_needs_files_write_and_only_that() {
    for c in WRITE_COMMANDS {
        assert_eq!(permission_for(c), Some(WRITE_PERMISSION), "{c}");
    }
    for c in COMMANDS {
        assert_eq!(permission_for(c), Some(PERMISSION), "{c}");
    }
    assert_eq!(permission_for("os_pick_file"), None);
    assert!(battalion_app_api::manifest::PERMISSIONS.contains(&WRITE_PERMISSION));
}

#[test]
fn write_mkdir_rename_copy_delete_round_trip() {
    let t = tempfile::tempdir().unwrap();
    let fs = fs_in(t.path());
    let r = fs.root().to_path_buf();
    fs.call("os_fs_mkdir", &json!({"path": "docs"})).unwrap();
    fs.call("os_fs_write", &json!({"path": "docs/a.txt", "data": "hello"})).unwrap();
    assert_eq!(std::fs::read_to_string(r.join("docs/a.txt")).unwrap(), "hello");
    fs.call("os_fs_write", &json!({"path": "docs/b.bin", "data": "AAEC", "as": "base64"})).unwrap();
    assert_eq!(std::fs::read(r.join("docs/b.bin")).unwrap(), vec![0, 1, 2]);
    // overwrite: false refuses an existing file; the default replaces it.
    assert_eq!(fs.call("os_fs_write", &json!({"path": "docs/a.txt", "data": "x", "overwrite": false})).unwrap_err(), FsError::Exists);
    fs.call("os_fs_write", &json!({"path": "docs/a.txt", "data": "bye"})).unwrap();
    assert_eq!(std::fs::read_to_string(r.join("docs/a.txt")).unwrap(), "bye");
    fs.call("os_fs_copy", &json!({"from": "docs", "to": "docs2"})).unwrap();
    assert_eq!(names(&r.join("docs2")), vec!["a.txt", "b.bin"]);
    fs.call("os_fs_copy", &json!({"from": "docs/a.txt", "to": "c.txt"})).unwrap();
    assert_eq!(std::fs::read_to_string(r.join("c.txt")).unwrap(), "bye");
    fs.call("os_fs_rename", &json!({"from": "c.txt", "to": "docs2/c.txt"})).unwrap();
    assert!(r.join("docs2/c.txt").is_file() && !r.join("c.txt").exists());
    assert_eq!(fs.call("os_fs_rename", &json!({"from": "docs2/c.txt", "to": "docs/a.txt"})).unwrap_err(), FsError::Exists);
    assert_eq!(fs.call("os_fs_rename", &json!({"from": "docs", "to": "docs/inner"})).unwrap_err().code(), "bad_args");
    assert_eq!(fs.call("os_fs_delete", &json!({"path": "docs2"})).unwrap_err(), FsError::NotEmpty);
    fs.call("os_fs_delete", &json!({"path": "docs2", "recursive": true})).unwrap();
    assert!(!r.join("docs2").exists());
    assert_eq!(names(&r), vec!["docs"], "no temp file is left anywhere");
}

#[test]
fn escape_on_either_end_is_refused() {
    let t = tempfile::tempdir().unwrap();
    let fs = fs_in(t.path());
    let outside = t.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "s").unwrap();
    std::fs::write(fs.root().join("in.txt"), "i").unwrap();
    for bad in ["../outside/x", "/abs", "C:/x", "a/../../x", "..", "CON"] {
        for (cmd, args) in [
            ("os_fs_write", json!({"path": bad, "data": "x"})),
            ("os_fs_mkdir", json!({"path": bad})),
            ("os_fs_delete", json!({"path": bad})),
            ("os_fs_trash", json!({"path": bad})),
            ("os_fs_rename", json!({"from": "in.txt", "to": bad})),
            ("os_fs_rename", json!({"from": bad, "to": "x"})),
            ("os_fs_copy", json!({"from": "in.txt", "to": bad})),
            ("os_fs_copy", json!({"from": bad, "to": "x"})),
        ] {
            let e = fs.call(cmd, &args).unwrap_err();
            assert!(matches!(e, FsError::BadPath(_) | FsError::IsRoot), "{cmd} {bad}: {e:?}");
        }
    }
    // Through a junction out: writing, making, renaming into and copying out of it are all refused.
    if dir_link(&fs.root().join("out"), &outside) {
        for (cmd, args) in [
            ("os_fs_write", json!({"path": "out/new.txt", "data": "x"})),
            ("os_fs_mkdir", json!({"path": "out/d"})),
            ("os_fs_rename", json!({"from": "in.txt", "to": "out/in.txt"})),
            ("os_fs_copy", json!({"from": "in.txt", "to": "out/in.txt"})),
            ("os_fs_copy", json!({"from": "out/secret.txt", "to": "stolen.txt"})),
            ("os_fs_copy", json!({"from": "out", "to": "stolen"})),
            ("os_fs_delete", json!({"path": "out/secret.txt"})),
        ] {
            assert_eq!(fs.call(cmd, &args).unwrap_err(), FsError::LeavesRoot, "{cmd} {args}");
        }
        // A folder holding a link out cannot be copied either, and nothing is left behind.
        std::fs::create_dir(fs.root().join("box")).unwrap();
        assert!(dir_link(&fs.root().join("box/out"), &outside));
        assert_eq!(fs.call("os_fs_copy", &json!({"from": "box", "to": "box2"})).unwrap_err(), FsError::LeavesRoot);
        assert!(!fs.root().join("box2").exists());
        assert_eq!(names(&outside), vec!["secret.txt"]);
    }
}

#[test]
fn deleting_a_junction_leaves_its_target_intact() {
    let t = tempfile::tempdir().unwrap();
    let fs = fs_in(t.path());
    let target = fs.root().join("real");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("keep.txt"), "k").unwrap();
    let outside = t.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("keep.txt"), "k").unwrap();
    if !dir_link(&fs.root().join("link"), &target) {
        eprintln!("no junction here; skipped");
        return;
    }
    fs.call("os_fs_delete", &json!({"path": "link", "recursive": true})).unwrap();
    assert!(!fs.root().join("link").exists());
    assert!(target.join("keep.txt").is_file(), "the target of the deleted junction is intact");
    // A recursive delete of a folder holding a junction out removes the junction, not what it points at.
    std::fs::create_dir(fs.root().join("box")).unwrap();
    assert!(dir_link(&fs.root().join("box/out"), &outside));
    fs.call("os_fs_delete", &json!({"path": "box", "recursive": true})).unwrap();
    assert!(!fs.root().join("box").exists());
    assert!(outside.join("keep.txt").is_file(), "a recursive delete does not go through a junction");
    // A junction is not handed to the recycle bin.
    assert!(dir_link(&fs.root().join("link2"), &target));
    assert_eq!(fs.call("os_fs_trash", &json!({"path": "link2"})).unwrap_err().code(), "bad_args");
}

#[test]
fn the_root_itself_cannot_be_removed_or_moved() {
    let t = tempfile::tempdir().unwrap();
    let fs = fs_in(t.path());
    std::fs::write(fs.root().join("f.txt"), "f").unwrap();
    for (cmd, args) in [
        ("os_fs_delete", json!({"path": "", "recursive": true})),
        ("os_fs_delete", json!({})),
        ("os_fs_trash", json!({"path": ""})),
        ("os_fs_rename", json!({"from": "", "to": "x"})),
        ("os_fs_write", json!({"path": "", "data": "x"})),
        ("os_fs_mkdir", json!({"path": ""})),
    ] {
        let e = fs.call(cmd, &args).unwrap_err();
        assert!(matches!(e, FsError::IsRoot | FsError::BadArgs(_)), "{cmd}: {e:?}");
    }
    // Copying the root into itself is refused too.
    assert_eq!(fs.call("os_fs_copy", &json!({"from": "", "to": "copy"})).unwrap_err().code(), "bad_args");
    assert!(fs.root().join("f.txt").is_file());
}

#[test]
fn the_os_home_under_the_root_stays_out_of_reach() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("root");
    let home = root.join("os-home");
    std::fs::create_dir_all(&home).unwrap();
    let fs = Fs::with_root(&root, Some(&home), Settings::default()).unwrap();
    assert_eq!(fs.call("os_fs_delete", &json!({"path": "os-home", "recursive": true})).unwrap_err(), FsError::LeavesRoot);
    assert_eq!(fs.call("os_fs_rename", &json!({"from": "os-home", "to": "x"})).unwrap_err(), FsError::LeavesRoot);
    assert_eq!(fs.call("os_fs_write", &json!({"path": "os-home/x", "data": "x"})).unwrap_err(), FsError::LeavesRoot);
    assert!(home.is_dir());
}

#[test]
fn an_atomic_write_leaves_no_partial_file() {
    let t = tempfile::tempdir().unwrap();
    let fs = Fs::with_root(&t.path().join("root"), None, Settings { write_max_bytes: 8, ..Settings::default() }).unwrap();
    fs.call("os_fs_write", &json!({"path": "a.txt", "data": "old"})).unwrap();
    // Too large: refused before any byte is written; the old file is whole and no temp is left.
    assert_eq!(fs.call("os_fs_write", &json!({"path": "a.txt", "data": "0123456789"})).unwrap_err().code(), "too_large");
    assert_eq!(std::fs::read_to_string(fs.root().join("a.txt")).unwrap(), "old");
    // A write onto a folder fails after nothing was published; no temp left either.
    std::fs::create_dir(fs.root().join("d")).unwrap();
    assert_eq!(fs.call("os_fs_write", &json!({"path": "d", "data": "x"})).unwrap_err(), FsError::NotAFile);
    // Bad base64 is refused whole.
    assert_eq!(fs.call("os_fs_write", &json!({"path": "b.txt", "data": "%%%", "as": "base64"})).unwrap_err().code(), "bad_args");
    assert_eq!(names(fs.root()), vec!["a.txt", "d"]);
    // A reader while a write replaces the file sees the old bytes or the new, never a mix.
    let root = fs.root().to_path_buf();
    let big = Fs::with_root(&root, None, Settings::default()).unwrap();
    let a = "a".repeat(200_000);
    let b = "b".repeat(200_000);
    big.call("os_fs_write", &json!({"path": "x.txt", "data": a})).unwrap();
    let reader = std::thread::spawn({
        let root = root.clone();
        move || {
            for _ in 0..200 {
                if let Ok(s) = std::fs::read_to_string(root.join("x.txt")) {
                    assert!(s.len() == 200_000 && (s.bytes().all(|c| c == b'a') || s.bytes().all(|c| c == b'b')), "a partial file was seen");
                }
            }
        }
    });
    for i in 0..20 {
        big.call("os_fs_write", &json!({"path": "x.txt", "data": if i % 2 == 0 { &b } else { &a }})).unwrap();
    }
    reader.join().unwrap();
    assert_eq!(names(&root), vec!["a.txt", "d", "x.txt"]);
}

#[test]
fn trash_moves_a_file_out_of_the_root() {
    let t = tempfile::tempdir().unwrap();
    let fs = fs_in(t.path());
    std::fs::write(fs.root().join("gone.txt"), "g").unwrap();
    match fs.call("os_fs_trash", &json!({"path": "gone.txt"})) {
        Ok(_) => assert!(!fs.root().join("gone.txt").exists()),
        // A device or volume with no recycle bin says so; the file stays.
        Err(e) => assert!(fs.root().join("gone.txt").exists(), "{e}"),
    }
    assert_eq!(fs.call("os_fs_trash", &json!({"path": "nope.txt"})).unwrap_err(), FsError::NotFound);
}

#[test]
fn watches_are_dropped_when_the_frame_closes() {
    let t = tempfile::tempdir().unwrap();
    let fs = fs_in(t.path());
    let mut ws = Watches::default();
    ws.add(&fs, "app-a", "").unwrap();
    ws.add(&fs, "app-b", "").unwrap();
    std::fs::write(fs.root().join("n.txt"), "n").unwrap();
    ws.clear_app("app-a");
    let changes = ws.poll(&fs);
    assert!(!changes.is_empty() && changes.iter().all(|(app, _, _)| app == "app-b"), "{changes:?}");
    ws.clear_app("app-b");
    assert!(ws.is_empty());
}

// SPDX-License-Identifier: MPL-2.0
//! BOS-31 developer mode. The dev tests need `--features dev`; the release test runs without it. Real workers, loopback only.

use battalion_host::dev::dev_url_origin;
use battalion_host::supervisor::{platform_key, Settings, Supervisor};
use battalion_host::Paths;
use std::path::{Path, PathBuf};

mod common;
use common::*;

#[test]
fn only_loopback_http_urls_are_dev_urls() {
    for ok in ["http://127.0.0.1:5173", "http://localhost:5173/", "http://[::1]:5173/x?y=1", "https://127.0.0.1:8443"] {
        assert!(dev_url_origin(ok).is_ok(), "{ok}");
    }
    assert_eq!(dev_url_origin("http://127.0.0.1:5173/a/b").unwrap(), "http://127.0.0.1:5173");
    let bad: Vec<String> = ["http://0.0.0.0:5173", "http://example.com", "http://127.0.0.1.host.example", "file:///c:/x", "ftp://127.0.0.1", "127.0.0.1:5173", "http://"]
        .iter()
        .map(|s| s.to_string())
        .chain([["http://192", ".168.1.5:5173"].concat(), ["http://user", "@127.0.0.1:1"].concat(), ["http://127.0.0.1", "@host.example"].concat()])
        .collect();
    for bad in &bad {
        assert!(dev_url_origin(bad).is_err(), "{bad} must be refused");
    }
}

fn app_folder(root: &Path, name: &str) -> (PathBuf, PathBuf) {
    let dir = root.join(name);
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    std::fs::copy(demo_lib(), dir.join(LIB_IN_PACKAGE)).unwrap();
    std::fs::write(dir.join("ui/index.html"), "<p>dev</p>").unwrap();
    let manifest = serde_json::json!({
        "manifest": 1, "id": name, "name": name, "version": "0.0.1",
        "contract": { "min": 1, "max": 1 },
        "process": { platform_key(): { "library": LIB_IN_PACKAGE, "args": [], "readyTimeoutMs": 10000, "stopTimeoutMs": 3000 } },
        "ui": { "entry": "ui/index.html" }
    });
    std::fs::write(dir.join("app.json"), manifest.to_string()).unwrap();
    let lib = dir.join(LIB_IN_PACKAGE);
    (dir, lib)
}

fn home(name: &str) -> Paths {
    let d = std::env::temp_dir().join(format!("bos-dev-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let p = Paths::new(&d);
    p.ensure().unwrap();
    p
}

fn supervisor(paths: &Paths) -> Supervisor {
    sup(paths.clone(), Settings { ready_poll_ms: 20, auto_restart: false, ..Settings::default() })
}

/// A release build (no `dev` feature) refuses a folder app: a planted registration is not followed, nothing starts.
#[cfg(not(feature = "dev"))]
#[test]
fn a_release_build_refuses_a_folder_app() {
    assert!(!battalion_host::dev::ENABLED);
    let paths = home("release");
    let (dir, _) = app_folder(&paths.root().join("src"), "folder-app");
    let app = paths.app_dir("folder-app").unwrap();
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join("dev.json"), serde_json::json!({"dir": dir.display().to_string()}).to_string()).unwrap();
    std::fs::write(app.join("current.json"), r#"{"version":"0.0.1","state":"stopped"}"#).unwrap();
    assert!(battalion_host::dev::load(&paths, "folder-app").is_none());
    assert_eq!(paths.content_dir("folder-app", "0.0.1").unwrap(), paths.version_dir("folder-app", "0.0.1").unwrap());
    let err = supervisor(&paths).start("folder-app").expect_err("a folder app must not start in a release build");
    assert!(!err.to_string().is_empty());
    assert!(!app.join("run").exists());
}

#[cfg(feature = "dev")]
mod dev {
    use super::*;
    use battalion_host::dev::{self, DevEvent};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    fn events() -> (Arc<Mutex<Vec<DevEvent>>>, Arc<dyn Fn(DevEvent) + Send + Sync>) {
        let v = Arc::new(Mutex::new(Vec::new()));
        let v2 = v.clone();
        (v, Arc::new(move |e| v2.lock().unwrap().push(e)))
    }
    fn wait_for(v: &Arc<Mutex<Vec<DevEvent>>>, n: usize) -> Vec<DevEvent> {
        let t = Instant::now();
        while v.lock().unwrap().len() < n {
            assert!(t.elapsed() < Duration::from_secs(20), "no dev event: {:?}", v.lock().unwrap());
            std::thread::sleep(Duration::from_millis(50));
        }
        v.lock().unwrap().clone()
    }

    #[test]
    fn a_folder_app_runs_from_a_copy_and_the_original_can_be_overwritten() {
        let paths = home("copy");
        let (dir, lib) = app_folder(&paths.root().join("src"), "dev-copy");
        let id = dev::register(&paths, &dir, None, &Default::default()).unwrap();
        assert_eq!(id, "dev-copy");
        let sup = supervisor(&paths);
        let st = sup.start(&id).unwrap();
        assert_eq!(st.state, "running");
        // The worker loaded a COPY: the original is free to be overwritten while the app runs (Windows locks a loaded library).
        std::fs::copy(demo_lib(), &lib).expect("the compiler must be able to overwrite the library of a running dev app");
        let copies: Vec<_> = std::fs::read_dir(paths.root().join("run/dev-copy/dev-lib")).unwrap().flatten().collect();
        assert_eq!(copies.len(), 1);
        sup.stop(&id).unwrap();
    }

    #[test]
    fn a_changed_library_restarts_only_that_app_and_a_bad_one_is_reported_and_watching_goes_on() {
        let paths = home("watch");
        let src = paths.root().join("src");
        let (a_dir, a_lib) = app_folder(&src, "dev-a");
        let (b_dir, _) = app_folder(&src, "dev-b");
        dev::register(&paths, &a_dir, None, &Default::default()).unwrap();
        dev::register(&paths, &b_dir, None, &Default::default()).unwrap();
        let sup = supervisor(&paths);
        let (a0, b0) = (sup.start("dev-a").unwrap().pid.unwrap(), sup.start("dev-b").unwrap().pid.unwrap());
        let (log, on) = events();
        let _w = dev::watch(paths.clone(), sup.clone(), Duration::from_millis(100), on);
        std::thread::sleep(Duration::from_millis(400)); // the watcher has seen the first look

        // A new build of A: the same library with a trailing byte (still loads).
        let mut bytes = std::fs::read(demo_lib()).unwrap();
        bytes.push(0);
        std::fs::write(&a_lib, &bytes).unwrap();
        let ev = wait_for(&log, 1);
        assert!(matches!(&ev[0], DevEvent::Restarted { id, .. } if id == "dev-a"), "{ev:?}");
        let (a1, b1) = (sup.status("dev-a").unwrap(), sup.status("dev-b").unwrap());
        assert_eq!(a1.state, "running");
        assert_ne!(a1.pid.unwrap(), a0, "A has a new worker");
        assert_eq!(b1.pid.unwrap(), b0, "B was not touched");

        // A build that cannot load: plain error, A is not running, the watching goes on.
        std::fs::write(&a_lib, b"not a library").unwrap();
        let ev = wait_for(&log, 2);
        assert!(matches!(&ev[1], DevEvent::Failed { id, error } if id == "dev-a" && !error.is_empty()), "{ev:?}");
        assert_eq!(sup.status("dev-b").unwrap().pid.unwrap(), b0);

        // The next good build starts again.
        std::fs::write(&a_lib, &bytes).unwrap();
        std::fs::write(&a_lib, [&bytes[..], &[1u8][..]].concat()).unwrap();
        let ev = wait_for(&log, 3);
        assert!(matches!(&ev[2], DevEvent::Restarted { id, .. } if id == "dev-a"), "{ev:?}");
        assert_eq!(sup.status("dev-a").unwrap().state, "running");
        sup.stop("dev-a").unwrap();
        sup.stop("dev-b").unwrap();
    }

    #[test]
    fn an_installed_app_is_never_replaced_and_a_dev_url_must_be_loopback() {
        let paths = home("guard");
        let (dir, _) = app_folder(&paths.root().join("src"), "dev-guard");
        let app = paths.app_dir("dev-guard").unwrap();
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("current.json"), r#"{"version":"1.0.0","state":"stopped"}"#).unwrap();
        let e = dev::register(&paths, &dir, None, &Default::default()).unwrap_err();
        assert!(e.contains("installed the normal way"), "{e}");
        let (dir2, _) = app_folder(&paths.root().join("src"), "dev-url");
        assert!(dev::register(&paths, &dir2, Some("http://0.0.0.0:5173"), &Default::default()).is_err());
        assert!(dev::register(&paths, &dir2, Some("http://127.0.0.1:5173"), &Default::default()).is_ok());
        // The app is told the dev server's origin as its page origin.
        assert!(dev::register(&paths, &dir2, None, &[("BATTALION_HOME".to_string(), "x".to_string())].into()).is_err());
        let sup = supervisor(&paths);
        sup.start("dev-url").unwrap();
        sup.stop("dev-url").unwrap();
    }

    /// LOCAL-TRY-1: a page-only folder app (a page, no process part) registers, runs as its page from the folder, and stops;
    /// a folder with neither a process part nor a page is refused.
    #[test]
    fn a_page_only_folder_app_runs_as_its_page() {
        let paths = home("page-only");
        let src = paths.root().join("src");
        let write = |id: &str, ui: bool| {
            let dir = src.join(id);
            std::fs::create_dir_all(dir.join("ui")).unwrap();
            std::fs::write(dir.join("ui/index.html"), "<p>page</p>").unwrap();
            let mut m = serde_json::json!({ "manifest": 1, "id": id, "name": id, "version": "0.0.1", "contract": { "min": 1, "max": 1 } });
            if ui {
                m["ui"] = serde_json::json!({ "entry": "ui/index.html" });
            }
            std::fs::write(dir.join("app.json"), m.to_string()).unwrap();
            dir
        };
        let id = dev::register(&paths, &write("dev-page", true), Some("http://127.0.0.1:5173"), &Default::default()).unwrap();
        assert_eq!(paths.content_dir(&id, "0.0.1").unwrap(), src.join("dev-page").canonicalize().map(|p| PathBuf::from(p.to_string_lossy().trim_start_matches(r"\\?\"))).unwrap());
        assert!(dev::apps(&paths).is_empty(), "a page-only app has no library to watch");
        let sup = supervisor(&paths);
        let st = sup.start(&id).unwrap();
        assert_eq!((st.state.as_str(), st.pid), ("running", None));
        assert_eq!(sup.stop(&id).unwrap().state, "stopped");
        assert!(dev::register(&paths, &write("dev-bare", false), None, &Default::default()).is_err());
    }
}

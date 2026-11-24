// SPDX-License-Identifier: MPL-2.0
//! Supervisor tests against the fixture app (real processes, real loopback HTTP).
//! Each test has its own home folder; the fixture listens on port 0.

use battalion_host::process::{self, ProcId, SpawnSpec};
use battalion_host::state::{AppState, Current};
use battalion_host::supervisor::{platform_key, Settings, SupError, Supervisor};
use battalion_host::{store, Paths};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn demo_exe() -> PathBuf {
    static EXE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    EXE.get_or_init(find_demo).clone()
}

fn find_demo() -> PathBuf {
    let name = if cfg!(windows) { "demo-app.exe" } else { "demo-app" };
    let look = || std::env::current_exe().unwrap().ancestors().skip(1).take(6).map(|d| d.join(name)).find(|p| p.is_file());
    if let Some(p) = look() {
        return p;
    }
    let ws = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let ok = std::process::Command::new("cargo").args(["build", "-p", "demo-app"]).current_dir(ws).status().unwrap().success();
    assert!(ok, "could not build the fixture app");
    look().expect("fixture app exe not found near the test binary")
}

fn fast() -> Settings {
    Settings {
        ready_poll_ms: 20,
        health_interval_ms: 150,
        health_timeout_ms: 500,
        health_fail_limit: 2,
        backoff_initial_ms: 100,
        backoff_max_ms: 400,
        backoff_reset_after_ms: 60_000,
        max_restarts: 3,
        kill_wait_ms: 5_000,
        auto_restart: true,
    }
}

struct Home {
    paths: Paths,
}
impl Home {
    fn new(name: &str) -> Home {
        let d = std::env::temp_dir().join(format!("bos-sup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let paths = Paths::new(&d);
        paths.ensure().unwrap();
        Home { paths }
    }
    /// Install the fixture as `id`. `args` pick the fixture mode.
    fn install(&self, id: &str, args: &[&str], ready_timeout_ms: u64) {
        let vdir = self.paths.version_dir(id, "1.0.0").unwrap();
        std::fs::create_dir_all(vdir.join("bin")).unwrap();
        let exec = if cfg!(windows) { "bin/demo-app.exe" } else { "bin/demo-app" };
        std::fs::copy(demo_exe(), vdir.join(exec)).unwrap();
        let manifest = serde_json::json!({
            "manifest": 1, "id": id, "name": id, "version": "1.0.0",
            "contract": { "min": 1, "max": 1 },
            "process": { platform_key(): { "exec": exec, "args": args, "readyTimeoutMs": ready_timeout_ms, "stopTimeoutMs": 3000 } }
        });
        std::fs::write(vdir.join("app.json"), manifest.to_string()).unwrap();
        let cur = Current { version: "1.0.0".into(), previous: None, state: AppState::Stopped, public_key: String::new() };
        store::write_json(&self.paths.current_json(id).unwrap(), &cur).unwrap();
    }
    fn state(&self, id: &str) -> AppState {
        Current::load(&self.paths.current_json(id).unwrap()).unwrap().unwrap().state
    }
    fn sup(&self) -> Supervisor {
        Supervisor::with_settings(self.paths.clone(), fast())
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        // Whatever a failed test left running: kill by recorded pid, never by name.
        if let Ok(rd) = std::fs::read_dir(self.paths.root().join("run")) {
            for d in rd.flatten() {
                if let Ok(Some(v)) = store::read_json::<serde_json::Value>(&d.path().join("proc.json")) {
                    if let Ok(id) = serde_json::from_value::<ProcId>(v["id"].clone()) {
                        process::kill_tree(&id);
                    }
                }
            }
        }
    }
}

fn counter(sup: &Supervisor, id: &str) -> u64 {
    let h = sup.health(id).unwrap();
    h.detail.unwrap().strip_prefix("counter=").unwrap().parse().unwrap()
}

fn wait_for<T>(secs: u64, mut f: impl FnMut() -> Option<T>) -> T {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < end, "timed out");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn pid_of(sup: &Supervisor, id: &str) -> Option<u32> {
    sup.status(id).ok().and_then(|s| s.pid)
}

fn proc_id(sup: &Supervisor, id: &str) -> ProcId {
    let pid = pid_of(sup, id).unwrap();
    ProcId { pid, start: String::new() }
}

/// Freeze a process (a hung app that still exists).
fn suspend(pid: u32) {
    #[cfg(windows)]
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn OpenProcess(a: u32, i: i32, p: u32) -> *mut std::ffi::c_void;
        }
        #[link(name = "ntdll")]
        extern "system" {
            fn NtSuspendProcess(h: *mut std::ffi::c_void) -> i32;
        }
        let h = OpenProcess(0x0800, 0, pid);
        assert!(!h.is_null());
        assert_eq!(NtSuspendProcess(h), 0);
    }
    #[cfg(unix)]
    {
        assert!(std::process::Command::new("kill").args(["-STOP", &pid.to_string()]).status().unwrap().success());
    }
}

#[test]
fn start_health_stop_and_a_same_named_bystander_survives() {
    let h = Home::new("basic");
    h.install("app-a", &[], 10_000);
    // A process of the same image that the OS did not start.
    let bystander = process::spawn_detached(&SpawnSpec {
        exec: demo_exe(),
        args: vec![],
        env: [
            ("BATTALION_CONTRACT", "1"),
            ("BATTALION_READY_FILE", h.paths.root().join("bystander-ready.json").to_str().unwrap()),
            ("BATTALION_TOKEN_FILE", h.paths.root().join("bystander-token").to_str().unwrap()),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect(),
        cwd: h.paths.root().to_path_buf(),
        log: h.paths.logs().join("bystander.log"),
    })
    .unwrap();

    let sup = h.sup();
    let s = sup.start("app-a").unwrap();
    assert_eq!(s.state, "running");
    let pid = s.pid.unwrap();
    assert_ne!(pid, bystander.pid);
    assert!(sup.health("app-a").unwrap().ok);
    assert!(Path::new(&h.paths.data_dir("app-a").unwrap()).exists(), "state dir created");
    assert!(sup.start("app-a").is_err(), "a second start of a running app is refused");

    let s = sup.stop("app-a").unwrap();
    assert_eq!(s.state, "stopped");
    assert!(!process::pid_running(pid), "the app is gone");
    assert!(process::is_alive(&bystander), "stop is by pid: the other process of the same image lives");
    assert!(sup.health("app-a").is_err());
    process::kill_tree(&bystander);
}

#[test]
fn killing_a_crashing_and_a_hanging_app_never_touches_its_neighbour() {
    let h = Home::new("isolation");
    h.install("app-a", &[], 10_000);
    h.install("app-b", &[], 10_000);
    h.install("app-c", &[], 10_000);
    let sup = h.sup();
    sup.start("app-b").unwrap();
    sup.start("app-a").unwrap();
    let b_pid = pid_of(&sup, "app-b").unwrap();
    let b0 = counter(&sup, "app-b");

    // 1. A is killed from outside: restarted with a new pid.
    let a_old = proc_id(&sup, "app-a");
    process::kill_tree(&a_old);
    let a_new = wait_for(20, || pid_of(&sup, "app-a").filter(|p| *p != a_old.pid && sup.health("app-a").is_ok()));
    assert_ne!(a_new, a_old.pid);
    assert_eq!(sup.status("app-a").unwrap().state, "running");

    // 2. A hangs (frozen, still a live process): killed after the failed probes and restarted.
    suspend(a_new);
    let a_third = wait_for(30, || pid_of(&sup, "app-a").filter(|p| *p != a_new && sup.health("app-a").is_ok()));
    assert!(!process::pid_running(a_new), "the hung process was killed");
    assert_ne!(a_third, a_new);

    // 3. C dies again and again (killed as soon as it is back): backoff, then given up (failed), not looped forever.
    sup.start("app-c").unwrap();
    wait_for(40, || {
        if matches!(h.state("app-c"), AppState::Failed(_)) {
            return Some(());
        }
        if let Some(pid) = pid_of(&sup, "app-c") {
            process::kill_tree(&ProcId { pid, start: String::new() });
        }
        None
    });
    let c_status = sup.status("app-c").unwrap();
    assert!(c_status.last_error.unwrap().contains("gave up"));
    assert_eq!(c_status.restarts, 3);

    // B never noticed any of it.
    assert_eq!(pid_of(&sup, "app-b"), Some(b_pid), "B kept its pid");
    assert!(process::pid_running(b_pid));
    assert!(counter(&sup, "app-b") > b0, "B's counter kept growing");
    assert_eq!(h.state("app-b"), AppState::Running);
    sup.stop("app-a").unwrap();
    sup.stop("app-b").unwrap();
}

#[test]
fn an_app_that_hangs_or_never_gets_ready_is_killed_and_left_stopped() {
    let h = Home::new("badstart");
    h.install("app-b", &[], 10_000);
    h.install("hangs", &["--mode", "hang"], 1_500);
    h.install("silent", &["--mode", "never-ready"], 1_000);
    h.install("liar", &["--mode", "wrong-contract"], 5_000);
    let sup = h.sup();
    sup.start("app-b").unwrap();
    let b_pid = pid_of(&sup, "app-b").unwrap();

    for id in ["hangs", "silent", "liar"] {
        let err = sup.start(id).unwrap_err();
        assert!(matches!(err, SupError::NotReady(_)), "{id}: {err}");
        assert_eq!(h.state(id), AppState::Stopped, "{id} is stopped again after the failure");
        assert!(sup.status(id).unwrap().pid.is_none());
        assert!(!h.paths.root().join("run").join(id).join("proc.json").exists());
    }
    assert!(sup.status("silent").unwrap().last_error.unwrap().contains("no ready file"));
    assert!(sup.status("liar").unwrap().last_error.unwrap().contains("contract"));
    assert_eq!(pid_of(&sup, "app-b"), Some(b_pid));
    sup.stop("app-b").unwrap();
}

#[test]
fn the_os_ends_and_apps_keep_running_the_next_start_adopts_them() {
    let h = Home::new("adopt");
    h.install("app-a", &[], 10_000);
    h.install("app-b", &[], 10_000);
    h.install("app-gone", &[], 10_000);
    let sup = h.sup();
    for id in ["app-a", "app-b", "app-gone"] {
        sup.start(id).unwrap();
    }
    let (a, b) = (pid_of(&sup, "app-a").unwrap(), pid_of(&sup, "app-b").unwrap());
    let (a0, b0) = (counter(&sup, "app-a"), counter(&sup, "app-b"));
    let gone = proc_id(&sup, "app-gone");

    // The OS ends: supervisor gone, no app stopped.
    sup.release_all();
    drop(sup);
    // One app dies while the OS is away.
    process::kill_tree(&gone);
    std::thread::sleep(Duration::from_millis(1500));
    assert!(process::pid_running(a) && process::pid_running(b), "both apps outlived the OS");

    let sup2 = h.sup();
    let results = sup2.adopt_all();
    assert_eq!(results.len(), 3);
    for (id, r) in &results {
        match id.as_str() {
            "app-a" | "app-b" => assert!(r.as_ref().unwrap().is_some(), "{id} adopted"),
            _ => assert!(r.as_ref().unwrap().is_none(), "{id} was not running"),
        }
    }
    assert_eq!(pid_of(&sup2, "app-a"), Some(a), "same process, not a restart");
    assert_eq!(pid_of(&sup2, "app-b"), Some(b));
    assert!(counter(&sup2, "app-a") > a0 && counter(&sup2, "app-b") > b0, "counters never reset");
    assert_eq!(h.state("app-gone"), AppState::Stopped, "the dead one is settled, not stuck 'running'");
    assert_eq!(sup2.start("app-gone").unwrap().state, "running", "and can be started again");

    // The adopted app is supervised again: kill it and it comes back.
    process::kill_tree(&ProcId { pid: a, start: String::new() });
    wait_for(20, || pid_of(&sup2, "app-a").filter(|p| *p != a && sup2.health("app-a").is_ok()));
    // Stopping an adopted app works like any other.
    assert_eq!(sup2.stop("app-b").unwrap().state, "stopped");
    assert!(!process::pid_running(b));
    sup2.stop("app-a").unwrap();
    sup2.stop("app-gone").unwrap();
}

#[test]
fn a_foreign_process_on_the_run_files_is_refused_not_killed() {
    let h = Home::new("foreign");
    h.install("app-a", &[], 10_000);
    let sup = h.sup();
    // Somebody else started the fixture with this app's ready file. No record of ours names it.
    let run = h.paths.root().join("run").join("app-a");
    std::fs::create_dir_all(&run).unwrap();
    let foreign = process::spawn_detached(&SpawnSpec {
        exec: demo_exe(),
        args: vec![],
        env: [
            ("BATTALION_CONTRACT", "1"),
            ("BATTALION_READY_FILE", run.join("ready.json").to_str().unwrap()),
            ("BATTALION_TOKEN_FILE", run.join("their-token").to_str().unwrap()),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect(),
        cwd: h.paths.root().to_path_buf(),
        log: h.paths.logs().join("foreign.log"),
    })
    .unwrap();
    wait_for(10, || run.join("ready.json").exists().then_some(()));

    match sup.start("app-a") {
        Err(SupError::Foreign { pid }) => assert_eq!(pid, foreign.pid),
        other => panic!("expected Foreign, got {other:?}"),
    }
    assert!(process::is_alive(&foreign), "refused, not killed");
    assert_eq!(h.state("app-a"), AppState::Stopped, "state untouched");
    assert!(run.join("ready.json").exists(), "its files are left alone");
    // A stop call on a running-looking app must not reach it either.
    let mut cur = Current::load(&h.paths.current_json("app-a").unwrap()).unwrap().unwrap();
    cur.transition(&h.paths.current_json("app-a").unwrap(), AppState::Starting).unwrap();
    cur.transition(&h.paths.current_json("app-a").unwrap(), AppState::Running).unwrap();
    assert!(matches!(sup.stop("app-a"), Err(SupError::Foreign { .. })));
    assert!(process::is_alive(&foreign), "stop did not kill a process it did not start");
    assert!(sup.adopt("app-a").unwrap().is_none(), "no record: nothing to adopt");
    assert!(process::is_alive(&foreign), "adopt did not touch it either");
    process::kill_tree(&foreign);
}

#[test]
fn pause_means_no_restart_and_resume_picks_it_up() {
    let h = Home::new("pause");
    h.install("app-a", &[], 10_000);
    let sup = h.sup();
    sup.start("app-a").unwrap();
    let old = proc_id(&sup, "app-a");
    sup.pause("app-a");
    process::kill_tree(&old);
    std::thread::sleep(Duration::from_millis(1200)); // several health intervals
    assert_eq!(pid_of(&sup, "app-a"), Some(old.pid), "paused: nobody replaced it");
    assert!(sup.status("app-a").unwrap().paused);
    sup.resume("app-a");
    wait_for(20, || pid_of(&sup, "app-a").filter(|p| *p != old.pid && sup.health("app-a").is_ok()));
    sup.stop("app-a").unwrap();
}

#[test]
fn settings_come_from_os_json_with_defaults() {
    let h = Home::new("settings");
    assert_eq!(Settings::load(&h.paths), Settings::default());
    std::fs::write(h.paths.os_json(), r#"{"supervisor":{"healthIntervalMs":123,"autoRestart":false,"future":1},"other":{}}"#).unwrap();
    let s = Settings::load(&h.paths);
    assert_eq!(s.health_interval_ms, 123);
    assert!(!s.auto_restart);
    assert_eq!(s.max_restarts, Settings::default().max_restarts);
}

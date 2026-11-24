// SPDX-License-Identifier: MPL-2.0
//! Supervisor tests against the demo library (real workers, real loopback HTTP, a fixture OS program).
//! Each test has its own home folder; the fixture listens on port 0.

use battalion_host::process::{self, ProcId};
use battalion_host::state::{AppState, Current};
use battalion_host::supervisor::{platform_key, Settings, SupError, Supervisor};
use battalion_host::{store, Paths};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

mod common;
use common::*;

fn fast() -> Settings {
    Settings {
        ready_poll_ms: 20,
        health_interval_ms: 150,
        health_timeout_ms: 1500,
        health_fail_limit: 3,
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
    /// Install the demo library as `id`. `args` pick the demo mode.
    fn install(&self, id: &str, args: &[&str], ready_timeout_ms: u64) {
        let vdir = self.paths.version_dir(id, "1.0.0").unwrap();
        lay_demo(&vdir);
        let manifest = serde_json::json!({
            "manifest": 1, "id": id, "name": id, "version": "1.0.0",
            "contract": { "min": 1, "max": 1 },
            "process": { platform_key(): { "library": LIB_IN_PACKAGE, "args": args, "readyTimeoutMs": ready_timeout_ms, "stopTimeoutMs": 3000 } }
        });
        std::fs::write(vdir.join("app.json"), manifest.to_string()).unwrap();
        let cur = Current { version: "1.0.0".into(), previous: None, state: AppState::Stopped, public_key: String::new() };
        store::write_json(&self.paths.current_json(id).unwrap(), &cur).unwrap();
    }
    fn state(&self, id: &str) -> AppState {
        Current::load(&self.paths.current_json(id).unwrap()).unwrap().unwrap().state
    }
    fn sup(&self) -> Supervisor {
        sup(self.paths.clone(), fast())
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

/// A process of the same program that the OS did not start: it sleeps.
fn bystander(secs: u32) -> std::process::Child {
    Command::new(demo_helper()).args(["--sleep", &secs.to_string()]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap()
}

fn child_pid(h: &Home, id: &str) -> u32 {
    wait_for(10, || std::fs::read_to_string(h.paths.data_dir(id).unwrap().join("child.pid")).ok().and_then(|t| t.trim().parse().ok()))
}

/// The helper's pid after a restart (the file holds the previous one until the app writes it again).
fn child_pid_fresh(h: &Home, id: &str, old: u32) -> u32 {
    wait_for(10, || std::fs::read_to_string(h.paths.data_dir(id).unwrap().join("child.pid")).ok().and_then(|t| t.trim().parse().ok()).filter(|p| *p != old))
}

/// End exactly this process (not its tree), the way an outside kill would.
fn kill_pid_only(pid: u32) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill").args(["/PID", &pid.to_string(), "/F"]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    #[cfg(unix)]
    {
        let _ = Command::new("kill").args(["-KILL", &pid.to_string()]).status();
    }
}

#[test]
fn start_health_stop_and_a_same_named_bystander_survives() {
    let h = Home::new("basic");
    h.install("app-a", &[], 10_000);
    let mut by = bystander(60);
    let sup = h.sup();
    let s = sup.start("app-a").unwrap();
    assert_eq!(s.state, "running");
    let pid = s.pid.unwrap();
    assert_ne!(pid, by.id());
    assert!(sup.health("app-a").unwrap().ok);
    assert!(Path::new(&h.paths.data_dir("app-a").unwrap()).exists(), "state dir created");
    assert!(sup.start("app-a").is_err(), "a second start of a running app is refused");

    let s = sup.stop("app-a").unwrap();
    assert_eq!(s.state, "stopped");
    assert!(!process::pid_running(pid), "the app is gone");
    assert!(process::pid_running(by.id()), "stop is by the app's own job: another process of the same program lives");
    assert!(sup.health("app-a").is_err());
    let _ = by.kill();
}

#[test]
fn the_worker_is_the_os_child_in_its_own_job_and_its_grandchild_ends_with_it() {
    let h = Home::new("job");
    h.install("kid", &["--mode", "child"], 10_000);
    let sup = h.sup();
    let s = sup.start("kid").unwrap();
    let worker = s.pid.unwrap();
    assert_eq!(parent_of(worker), Some(std::process::id()), "the worker is a child of the OS (here: this test)");
    assert!(in_job(worker), "the worker is in its job");
    let grandchild = child_pid(&h, "kid");
    assert_eq!(parent_of(grandchild), Some(worker), "the app's own helper is the worker's child");
    assert!(in_job(grandchild), "and it joined the app's job");
    assert!(process::pid_running(grandchild));
    // A clean stop: the worker ends, and the group goes with it.
    sup.stop("kid").unwrap();
    wait_for(10, || (!process::pid_running(worker) && !process::pid_running(grandchild)).then_some(()));
    // Killing the worker from outside also takes the group (auto restart is on in `fast`, so a new worker follows).
    sup.start("kid").unwrap();
    let w2 = pid_of(&sup, "kid").unwrap();
    let g2 = child_pid_fresh(&h, "kid", grandchild);
    kill_pid_only(w2);
    wait_for(10, || (!process::pid_running(g2)).then_some(()));
    sup.pause("kid");
    let _ = sup.stop("kid");
}

/// Run the fixture OS (`serve`) on a home with the given apps; returns it, each app's worker pid and its own pid.
fn fixture_os_serving(h: &Home, ids: &[&str]) -> (std::process::Child, std::collections::HashMap<String, u32>, u32) {
    let mut cmd = Command::new(fixture_os());
    cmd.arg("serve").arg(h.paths.root()).args(ids).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut os = cmd.spawn().unwrap();
    let mut lines = BufReader::new(os.stdout.take().unwrap());
    let (mut apps, mut os_pid) = (std::collections::HashMap::new(), 0);
    loop {
        let mut l = String::new();
        assert!(lines.read_line(&mut l).unwrap() > 0, "the fixture OS ended early");
        let f: Vec<&str> = l.split_whitespace().collect();
        match f.as_slice() {
            ["UP", id, pid] => {
                apps.insert(id.to_string(), pid.parse().unwrap());
            }
            ["FAIL", ..] => panic!("fixture OS: {l}"),
            ["READY", pid] => {
                os_pid = pid.parse().unwrap();
                break;
            }
            _ => {}
        }
    }
    // Keep reading its stdout so it never blocks on a full pipe.
    std::thread::spawn(move || {
        let mut sink = String::new();
        while lines.read_line(&mut sink).unwrap_or(0) > 0 {
            sink.clear();
        }
    });
    (os, apps, os_pid)
}

#[test]
fn when_the_os_is_killed_its_apps_and_their_children_end() {
    let h = Home::new("oskill");
    h.install("kid", &["--mode", "child"], 10_000);
    let (mut os, apps, os_pid) = fixture_os_serving(&h, &["kid"]);
    let worker = apps["kid"];
    let grandchild = child_pid(&h, "kid");
    assert!(process::pid_running(worker) && process::pid_running(grandchild));
    // The OS is killed (no clean exit): by its pid, nothing else.
    kill_pid_only(os_pid);
    let _ = os.wait();
    wait_for(15, || (!process::pid_running(worker) && !process::pid_running(grandchild)).then_some(()));
}

#[test]
fn exit_stops_every_app_cleanly_then_the_os_ends() {
    let h = Home::new("osexit");
    h.install("kid", &["--mode", "child"], 10_000);
    h.install("plain", &[], 10_000);
    let (mut os, apps, _) = fixture_os_serving(&h, &["kid", "plain"]);
    let grandchild = child_pid(&h, "kid");
    writeln!(os.stdin.as_mut().unwrap(), "exit").unwrap();
    let status = os.wait().unwrap();
    assert!(status.success());
    for (id, pid) in &apps {
        assert!(!process::pid_running(*pid), "{id} ended at Exit");
        assert_eq!(h.state(id), AppState::Stopped, "{id}");
    }
    wait_for(10, || (!process::pid_running(grandchild)).then_some(()));
}

#[test]
fn the_worker_refuses_to_run_when_the_os_did_not_start_it() {
    let h = Home::new("byhand");
    h.install("app-a", &[], 10_000);
    let lib = h.paths.version_dir("app-a", "1.0.0").unwrap().join(LIB_IN_PACKAGE);
    // By hand: no handshake at all.
    let out = Command::new(fixture_os()).arg("--bos-app-worker").arg(&lib).stdin(Stdio::null()).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("started by BattalionOS only"));
    // By hand with a made-up nonce on one side only.
    let mut c = Command::new(fixture_os()).arg("--bos-app-worker").arg(&lib).env("BATTALION_WORKER_NONCE", "abc").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
    writeln!(c.stdin.as_mut().unwrap(), "wrong").unwrap();
    let out = c.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("handshake"));
}

#[test]
fn the_os_own_environment_is_not_the_apps_but_the_users_tools_are() {
    let h = Home::new("env");
    h.install("envy", &["--mode", "env"], 10_000);
    // The manifest names the prefix to scrub (the OS knows no app's names); BATTALION_ is always scrubbed.
    let mp = h.paths.version_dir("envy", "1.0.0").unwrap().join("app.json");
    let mut m: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&mp).unwrap()).unwrap();
    m["process"][platform_key()]["envScrub"] = serde_json::json!(["ZZAPP_"]);
    std::fs::write(&mp, m.to_string()).unwrap();
    // The worker inherits this process's environment: plant one of each family and a plain one.
    std::env::set_var("ZZAPP_TEST_LEAK", "1");
    std::env::set_var("BATTALION_TEST_LEAK", "1");
    std::env::set_var("BOS_USER_TOOL", "kept");
    let sup = h.sup();
    sup.start("envy").unwrap();
    let seen = std::fs::read_to_string(h.paths.data_dir("envy").unwrap().join("env.txt")).unwrap();
    assert!(seen.contains("BOS_USER_TOOL=kept"), "{seen}");
    assert!(!seen.contains("ZZAPP_TEST_LEAK") && !seen.contains("BATTALION_TEST_LEAK"), "{seen}");
    assert!(seen.contains("BATTALION_APP_ID=envy"), "contract values are set: {seen}");
    sup.stop("envy").unwrap();
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
        // (A start that fails passes through Failed for a moment; only "gave up" is the end.)
        if matches!(h.state("app-c"), AppState::Failed(_)) && sup.status("app-c").is_ok_and(|s| s.last_error.is_some_and(|e| e.contains("gave up"))) {
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

fn write_record(run: &Path, pid: u32, start: &str) {
    std::fs::create_dir_all(run).unwrap();
    std::fs::write(
        run.join("proc.json"),
        serde_json::json!({"id": {"pid": pid, "start": start}, "version": "1.0.0", "url": "http://127.0.0.1:1", "health": "/health", "stop_timeout_ms": 3000}).to_string(),
    )
    .unwrap();
}

#[test]
fn a_leftover_recorded_by_an_older_run_is_stopped_by_pid_and_start_time_never_by_name() {
    let h = Home::new("leftover");
    h.install("old", &[], 10_000);
    h.install("other", &[], 10_000);
    let sup = h.sup();
    sup.start("other").unwrap();
    let other = pid_of(&sup, "other").unwrap();
    // An older run's process, recorded with its start time; and a same-program process that is not recorded.
    let mut leftover = bystander(60);
    let mut by = bystander(60);
    let start = process::start_token_of(leftover.id()).unwrap();
    let run = h.paths.root().join("run").join("old");
    write_record(&run, leftover.id(), &start);
    let cp = h.paths.current_json("old").unwrap();
    let mut cur = Current::load(&cp).unwrap().unwrap();
    cur.transition(&cp, AppState::Starting).unwrap();
    cur.transition(&cp, AppState::Running).unwrap();

    let next = h.sup();
    // (One app's recovery only: `other` belongs to the live supervisor of this run, not to an older one.)
    assert_eq!(next.recover("old").unwrap(), true);
    let _ = leftover.wait();
    assert!(!process::pid_running(leftover.id()), "the recorded leftover was stopped");
    assert!(process::pid_running(by.id()), "the same program, not recorded: not touched");
    assert_eq!(h.state("old"), AppState::Stopped);
    assert!(!run.join("proc.json").exists());
    // A record whose start time does not match (a reused pid) is not ours: left alone.
    write_record(&run, by.id(), "1");
    let _ = next.recover("old");
    assert!(process::pid_running(by.id()), "pid reused by another process: not touched");
    assert_eq!(pid_of(&sup, "other"), Some(other), "another app of ours is not touched");
    sup.stop("other").unwrap();
    let _ = by.kill();
}

#[test]
fn a_foreign_process_on_the_run_files_is_refused_not_killed() {
    let h = Home::new("foreign");
    h.install("app-a", &[], 10_000);
    let sup = h.sup();
    // Somebody else's process is named by this app's ready file. No record of ours names it.
    let run = h.paths.root().join("run").join("app-a");
    std::fs::create_dir_all(&run).unwrap();
    let mut foreign = bystander(60);
    std::fs::write(run.join("ready.json"), serde_json::json!({"contract": 1, "pid": foreign.id(), "url": "http://127.0.0.1:9"}).to_string()).unwrap();

    match sup.start("app-a") {
        Err(SupError::Foreign { pid }) => assert_eq!(pid, foreign.id()),
        other => panic!("expected Foreign, got {other:?}"),
    }
    assert!(process::pid_running(foreign.id()), "refused, not killed");
    assert_eq!(h.state("app-a"), AppState::Stopped, "state untouched");
    assert!(run.join("ready.json").exists(), "its files are left alone");
    // A stop call on a running-looking app must not reach it either.
    let cp = h.paths.current_json("app-a").unwrap();
    let mut cur = Current::load(&cp).unwrap().unwrap();
    cur.transition(&cp, AppState::Starting).unwrap();
    cur.transition(&cp, AppState::Running).unwrap();
    assert!(matches!(sup.stop("app-a"), Err(SupError::Foreign { .. })));
    assert!(process::pid_running(foreign.id()), "stop did not kill a process it did not start");
    assert_eq!(sup.recover("app-a").ok(), Some(true), "the stale state is settled");
    assert!(process::pid_running(foreign.id()), "recover did not touch it either");
    let _ = foreign.kill();
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

#[test]
fn an_app_that_aborts_or_returns_an_error_leaves_the_os_and_another_app_running() {
    let h = Home::new("crash");
    h.install("steady", &[], 10_000);
    h.install("aborts", &["--mode", "abort", "--delay-ms", "2500"], 10_000);
    h.install("fails", &["--mode", "crash", "--delay-ms", "2500"], 10_000);
    let sup = h.sup();
    sup.start("steady").unwrap();
    let (steady, c0) = (pid_of(&sup, "steady").unwrap(), counter(&sup, "steady"));
    for id in ["aborts", "fails"] {
        let first = sup.start(id).unwrap().pid.unwrap();
        // The worker died with the app (abort) or ended with its code (run returned 1): restarted by the existing rules.
        let again = wait_for(30, || pid_of(&sup, id).filter(|p| *p != first && sup.health(id).is_ok()));
        assert_ne!(again, first, "{id} restarted with a new worker");
        assert!(sup.status(id).unwrap().restarts >= 1, "{id}");
        sup.pause(id);
    }
    assert_eq!(pid_of(&sup, "steady"), Some(steady), "the other app kept its worker");
    assert!(counter(&sup, "steady") > c0, "and kept running");
    assert_eq!(h.state("steady"), AppState::Running);
    for id in ["aborts", "fails", "steady"] {
        let _ = sup.stop(id);
    }
}

/// An app with a page: the OS names the page's origin before the app starts; the app lets exactly that origin reach its socket
/// (101) and refuses every other (403); the port is the same after a stop and a new start and after the OS restarts.
#[test]
fn an_app_with_a_page_gets_one_origin_that_stays_and_only_that_origin_gets_in() {
    use std::io::Read;
    let h = Home::new("uiorigin");
    h.install("app-ui", &[], 10_000);
    let mp = h.paths.version_dir("app-ui", "1.0.0").unwrap().join("app.json");
    let mut m: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&mp).unwrap()).unwrap();
    m["ui"] = serde_json::json!({"entry": "ui/index.html"});
    std::fs::write(&mp, m.to_string()).unwrap();
    h.install("app-plain", &[], 10_000); // no page: no origin

    let ws = |url: &str, origin: Option<&str>| -> String {
        let addr = url.trim_start_matches("http://").trim_end_matches('/').to_string();
        let mut c = std::net::TcpStream::connect(&addr).unwrap();
        let o = origin.map(|o| format!("Origin: {o}\r\n")).unwrap_or_default();
        write!(c, "GET /ws HTTP/1.1\r\nHost: {addr}\r\n{o}Connection: close\r\n\r\n").unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).unwrap();
        out.lines().next().unwrap_or("").to_string()
    };
    let sup = h.sup();
    let url = sup.start("app-ui").unwrap().url.unwrap();
    let origin = battalion_host::uiport::origin_for(&h.paths, "app-ui").unwrap();
    assert!(ws(&url, Some(&origin)).contains("101"), "its own page gets in");
    assert!(ws(&url, Some("http://127.0.0.1:1")).contains("403"), "another page is refused");
    assert!(ws(&url, Some("http://evil.example")).contains("403"));
    assert!(ws(&url, None).contains("403"), "no origin is refused");

    sup.stop("app-ui").unwrap();
    let url2 = h.sup().start("app-ui").unwrap().url.unwrap();
    assert_eq!(battalion_host::uiport::origin_for(&h.paths, "app-ui").unwrap(), origin, "same origin after a restart of the OS and the app");
    assert!(ws(&url2, Some(&origin)).contains("101"));
    h.sup().stop("app-ui").unwrap();

    let url3 = h.sup().start("app-plain").unwrap().url.unwrap();
    assert!(ws(&url3, Some(&origin)).contains("403"), "an app without a page was told no origin: nothing gets in");
    assert!(!h.paths.ui_port_file("app-plain").unwrap().exists(), "no port is kept for an app without a page");
    h.sup().stop("app-plain").unwrap();
}

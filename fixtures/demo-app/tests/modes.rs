// SPDX-License-Identifier: Apache-2.0
//! Drives the fixture the way the OS will: BATTALION_* env, ready file, health, stop.

use battalion_app_api::{env, Health, ReadyFile};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Run {
    child: Child,
    ready: PathBuf,
    dir: PathBuf,
}
impl Drop for Run {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(mode: &str, extra: &[&str]) -> Run {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!("bos-demo-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let ready = dir.join("ready.json");
    std::fs::write(dir.join("token"), "s3cret").unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_demo-app"))
        .args(["--mode", mode])
        .args(extra)
        .env(env::CONTRACT, "1")
        .env(env::STATE_DIR, dir.join("state"))
        .env(env::READY_FILE, &ready)
        .env(env::TOKEN_FILE, dir.join("token"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    Run { child, ready, dir }
}

fn wait_ready(r: &Run, within: Duration) -> Option<String> {
    let t = Instant::now();
    while t.elapsed() < within {
        if let Ok(s) = std::fs::read_to_string(&r.ready) {
            return Some(s);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

fn http(url: &str, req: &str, timeout: Duration) -> Option<(u16, String)> {
    let addr = url.trim_start_matches("http://");
    let mut s = TcpStream::connect(addr).ok()?;
    s.set_read_timeout(Some(timeout)).ok()?;
    write!(s, "{req}\r\nHost: x\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    let code = out.split_whitespace().nth(1)?.parse().ok()?;
    Some((code, out.split("\r\n\r\n").nth(1).unwrap_or("").to_string()))
}

#[test]
fn normal_is_ready_healthy_and_stops_only_with_the_token() {
    let mut r = start("normal", &[]);
    let ready = ReadyFile::parse(&wait_ready(&r, Duration::from_secs(10)).expect("ready file"), 1).unwrap();
    assert_eq!(ready.pid, r.child.id());
    let (code, body) = http(&ready.url, "GET /health HTTP/1.1", Duration::from_secs(2)).unwrap();
    let h: Health = serde_json::from_str(&body).unwrap();
    assert!(code == 200 && h.ok && h.busy == Some(0) && h.detail.unwrap().starts_with("counter="));
    assert_eq!(http(&ready.url, "POST /stop HTTP/1.1", Duration::from_secs(2)).unwrap().0, 401, "no bearer, no stop");
    assert!(r.child.try_wait().unwrap().is_none());
    let (code, _) = http(&ready.url, "POST /stop HTTP/1.1\r\nAuthorization: Bearer s3cret", Duration::from_secs(2)).unwrap();
    assert_eq!(code, 200);
    let t = Instant::now();
    while r.child.try_wait().unwrap().is_none() {
        assert!(t.elapsed() < Duration::from_secs(5), "did not exit after stop");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(r.child.try_wait().unwrap().unwrap().success());
}

#[test]
fn slow_is_not_ready_at_first() {
    let r = start("slow", &["--delay-ms", "800"]);
    std::thread::sleep(Duration::from_millis(250));
    assert!(!r.ready.exists());
    assert!(wait_ready(&r, Duration::from_secs(10)).is_some());
}

#[test]
fn hang_is_ready_but_never_answers() {
    let r = start("hang", &[]);
    let ready = ReadyFile::parse(&wait_ready(&r, Duration::from_secs(10)).unwrap(), 1).unwrap();
    assert!(http(&ready.url, "GET /health HTTP/1.1", Duration::from_millis(400)).is_none());
}

#[test]
fn crash_exits_with_failure_after_ready() {
    let mut r = start("crash", &["--delay-ms", "100"]);
    assert!(wait_ready(&r, Duration::from_secs(10)).is_some());
    let t = Instant::now();
    let status = loop {
        if let Some(s) = r.child.try_wait().unwrap() {
            break s;
        }
        assert!(t.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.code(), Some(1));
}

#[test]
fn never_ready_runs_without_a_ready_file() {
    let mut r = start("never-ready", &[]);
    assert!(wait_ready(&r, Duration::from_millis(600)).is_none());
    assert!(r.child.try_wait().unwrap().is_none(), "it keeps running: the OS must time it out");
}

#[test]
fn wrong_contract_is_refused_by_the_ready_parser() {
    let r = start("wrong-contract", &[]);
    let text = wait_ready(&r, Duration::from_secs(10)).unwrap();
    assert!(matches!(ReadyFile::parse(&text, 1), Err(battalion_app_api::ReadyError::WrongContract { expected: 1, got: 2 })));
}

#[test]
fn init_creates_its_marker_in_the_state_dir() {
    let r = start("normal", &["--init"]);
    let mut r = r;
    assert!(r.child.wait().unwrap().success());
    assert!(r.dir.join("state").join("init.done").exists());
}

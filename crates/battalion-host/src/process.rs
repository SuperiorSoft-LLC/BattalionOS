// SPDX-License-Identifier: MPL-2.0
//! One OS process, no policy: start an app worker as the OS's own child (in a job, stdin held), tell
//! whether a pid is the process we started, call it over loopback HTTP, kill a leftover by pid and
//! start time. Knows no app.
//!
//! A pid alone is not an identity (pids are reused), so a process is a `ProcId { pid, start }`:
//! the pid plus the time the OS says it began. A pid whose start time differs is another process.
//! Nothing here ever kills by image name.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcId {
    pub pid: u32,
    /// The OS's start time of the process, as an opaque string. Empty = could not be read.
    pub start: String,
}

pub struct WorkerSpec {
    /// The OS's own program, started with the internal worker argument.
    pub exe: PathBuf,
    pub library: PathBuf,
    pub env: Vec<(String, String)>,
    /// Names removed from the inherited environment (the OS's own BATTALION_* and the manifest's envScrub prefixes).
    pub env_remove: Vec<String>,
    pub cwd: PathBuf,
    /// stdout and stderr are appended here.
    pub log: PathBuf,
    /// The app's context JSON (one line).
    pub context: String,
}

/// An app worker the OS started and owns: its child process, the write end of its stdin and the
/// job it lives in. Dropping the last reference ends the worker and everything it started.
pub struct Worker {
    pub id: ProcId,
    child: std::sync::Mutex<std::process::Child>,
    stdin: std::sync::Mutex<Option<std::process::ChildStdin>>,
    job: crate::job::Job,
}

impl Worker {
    /// Tell the app to stop: the worker sees end of file on its stdin, calls the app's `stop`.
    pub fn close_stdin(&self) {
        *self.stdin.lock().unwrap() = None;
    }
    /// End the worker and its whole group now.
    pub fn kill(&self) {
        self.job.kill();
        let _ = self.child.lock().unwrap().kill();
    }
    /// The exit code once it has ended (also reaps it).
    pub fn try_exit(&self) -> Option<i32> {
        self.child.lock().unwrap().try_wait().ok().flatten().map(|s| s.code().unwrap_or(-1))
    }
}

/// Start the app worker as the OS's own child: no console window, its own process group, in its
/// own job from before it reads its first line (it does nothing until the handshake below), with
/// its stdin held by us.
pub fn spawn_worker(spec: &WorkerSpec) -> io::Result<Worker> {
    if let Some(d) = spec.log.parent() {
        std::fs::create_dir_all(d)?;
    }
    let log = std::fs::OpenOptions::new().create(true).append(true).open(&spec.log)?;
    let nonce = random_token();
    let mut cmd = Command::new(&spec.exe);
    cmd.arg(crate::worker::ARG).arg(&spec.library).current_dir(&spec.cwd).stdin(Stdio::piped()).stdout(log.try_clone()?).stderr(log);
    for k in &spec.env_remove {
        cmd.env_remove(k);
    }
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    cmd.env(crate::worker::NONCE_ENV, &nonce);
    group(&mut cmd);
    let job = crate::job::Job::new()?;
    let mut child = cmd.spawn()?;
    let pid = child.id();
    // The worker is blocked on its stdin: nothing of the app exists yet. Put it in the job first.
    if let Err(e) = job.assign(pid) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(e);
    }
    let mut stdin = child.stdin.take().expect("piped stdin");
    if let Err(e) = writeln!(stdin, "{nonce}").and_then(|_| writeln!(stdin, "{}", spec.context)).and_then(|_| stdin.flush()) {
        job.kill();
        let _ = child.wait();
        return Err(e);
    }
    Ok(Worker { id: ProcId { pid, start: start_token(pid).unwrap_or_default() }, child: std::sync::Mutex::new(child), stdin: std::sync::Mutex::new(Some(stdin)), job })
}

#[cfg(windows)]
fn group(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
}
#[cfg(unix)]
fn group(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0); // its own group: the job
}

/// A helper command (taskkill, ps, kill) with no console window.
fn quiet(program: &str) -> Command {
    let mut c = Command::new(program);
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct FileTime {
        pub lo: u32,
        pub hi: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        pub fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        pub fn CloseHandle(h: *mut c_void) -> i32;
        pub fn GetExitCodeProcess(h: *mut c_void, code: *mut u32) -> i32;
        pub fn GetProcessTimes(h: *mut c_void, c: *mut FileTime, e: *mut FileTime, k: *mut FileTime, u: *mut FileTime) -> i32;
    }
    pub const QUERY_LIMITED: u32 = 0x1000;
    pub const STILL_ACTIVE: u32 = 259;
}

/// `None` when the process does not exist (or cannot be queried).
#[cfg(windows)]
fn start_token(pid: u32) -> Option<String> {
    use sys::*;
    unsafe {
        let h = OpenProcess(QUERY_LIMITED, 0, pid);
        if h.is_null() {
            return None;
        }
        let (mut c, mut e, mut k, mut u) = (FileTime::default(), FileTime::default(), FileTime::default(), FileTime::default());
        let ok = GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) != 0;
        let mut code = 0u32;
        let got = GetExitCodeProcess(h, &mut code) != 0;
        CloseHandle(h);
        if !ok || !got || code != STILL_ACTIVE {
            return None;
        }
        Some((((c.hi as u64) << 32) | c.lo as u64).to_string())
    }
}

#[cfg(unix)]
fn start_token(pid: u32) -> Option<String> {
    let out = Command::new("ps").args(["-o", "stat=,lstart=", "-p", &pid.to_string()]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || text.is_empty() || text.starts_with('Z') {
        return None; // gone, or a zombie
    }
    Some(text.split_once(char::is_whitespace).map(|(_, rest)| rest.trim().to_string()).unwrap_or_default())
}

/// The OS's start-time token of a running process (what a record keeps to tell it from a reused pid).
pub fn start_token_of(pid: u32) -> Option<String> {
    start_token(pid)
}

/// The process exists, is running, and is the same one that was identified (same start time).
pub fn is_alive(id: &ProcId) -> bool {
    match start_token(id.pid) {
        None => false,
        Some(now) => id.start.is_empty() || now == id.start,
    }
}

/// A process we did not start, by pid only: used to say "foreign process, not touched".
pub fn pid_running(pid: u32) -> bool {
    start_token(pid).is_some()
}

/// Kill the process and everything it started, by pid. Never by image name.
pub fn kill_tree(id: &ProcId) {
    if !is_alive(id) {
        return; // the pid may belong to another process now: do not touch it
    }
    #[cfg(windows)]
    {
        let _ = quiet("taskkill").args(["/PID", &id.pid.to_string(), "/T", "/F"]).status();
    }
    #[cfg(unix)]
    {
        // The process leads its own group (see `group`): signal the whole group.
        let _ = quiet("kill").args(["-KILL", "--", &format!("-{}", id.pid)]).status();
        let _ = quiet("kill").args(["-KILL", &id.pid.to_string()]).status();
    }
}

/// Wait until the process is gone or `timeout` passed. True = gone.
pub fn wait_gone(id: &ProcId, timeout: Duration) -> bool {
    let end = Instant::now() + timeout;
    loop {
        if !is_alive(id) {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// One loopback HTTP/1.1 call. Returns (status, body). `base` is the ready file's `url`.
/// Anything slower than `timeout` in total is an error: a hung app must not hang the OS.
pub fn http(method: &str, base: &str, path: &str, bearer: Option<&str>, timeout: Duration) -> io::Result<(u16, String)> {
    let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidInput, m.to_string());
    let rest = base.strip_prefix("http://").ok_or_else(|| bad("only http on loopback"))?;
    let hostport = rest.split('/').next().unwrap_or(rest);
    let addr = hostport.to_socket_addrs()?.next().ok_or_else(|| bad("no address"))?;
    if !addr.ip().is_loopback() {
        return Err(bad("not a loopback address"));
    }
    let deadline = Instant::now() + timeout;
    let mut s = TcpStream::connect_timeout(&addr, timeout)?;
    s.set_write_timeout(Some(timeout))?;
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {hostport}\r\nConnection: close\r\nContent-Length: 0\r\n");
    if let Some(t) = bearer {
        req.push_str(&format!("Authorization: Bearer {t}\r\n"));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes())?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "no answer in time"));
        }
        s.set_read_timeout(Some(left))?;
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > 1 << 20 {
                    return Err(bad("answer too large"));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock => {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "no answer in time"))
            }
            Err(e) => return Err(e),
        }
    }
    let text = String::from_utf8_lossy(&buf).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head.split_whitespace().nth(1).and_then(|c| c.parse::<u16>().ok()).ok_or_else(|| bad("not an http answer"))?;
    Ok((status, body.to_string()))
}

/// Last bytes of a log file, for an error message.
pub fn log_tail(path: &Path, max: usize) -> String {
    let b = std::fs::read(path).unwrap_or_default();
    let from = b.len().saturating_sub(max);
    String::from_utf8_lossy(&b[from..]).trim().to_string()
}

/// 32 hex characters from the OS-seeded hasher: a per-start secret for the stop call.
pub fn random_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for i in 0..2u64 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(i ^ std::process::id() as u64);
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

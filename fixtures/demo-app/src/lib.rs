// SPDX-License-Identifier: Apache-2.0
//! Fixture app for contract v1, as a library (app ABI 1). BattalionOS runs it on its own worker:
//! `battalion_app_run` blocks until `battalion_app_stop` is called.
//!
//! Modes (`--mode <m>` in the manifest's `args`, or env `DEMO_MODE`):
//!   normal          ready at once; answers health; ends when told to stop
//!   slow            writes the ready file after `--delay-ms` (default 2000)
//!   hang            writes the ready file, then answers nothing and ignores stop
//!   crash           writes the ready file, then `run` returns code 1 after `--delay-ms` (default 500)
//!   abort           writes the ready file, then the process aborts after `--delay-ms` (default 500)
//!   never-ready     runs, never writes the ready file
//!   wrong-contract  writes a ready file that says contract 2
//!   env             like normal, and writes its environment to `env.txt` in the state dir
//!   child           like normal, and starts `bin/demo-helper --sleep 120` as its own child; the helper's pid is
//!                   written to `child.pid` in the state dir
//!   prepare         like normal, names `prepare` in its ready file (UPDATE-LIVE-1) and stays busy (health `busy` 1):
//!                   a prepare call answers "not ready" until `--delay-ms` (default 1500) after the first one, `now`
//!                   answers ready at once, `cancel` resets it. Each call appends a line (`wait`, `ready`, `now`,
//!                   `cancel`, `refused`) to `prepare.log` in the state dir
//!   prepare-never   like prepare, but never ready unless asked with `now`
//!
//! Health detail is `counter=<n>`: it grows once a second, so a test can see that another app
//! kept running (same pid, counter still growing) while this one was killed, hung or updated.

use battalion_app_api::abi::{Context, ABI_VERSION};
use battalion_app_api::{Health, ReadyFile};
use std::ffi::CStr;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::os::raw::c_char;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// UPDATE-LIVE-1 fixture: what a prepare call needs (the secret, the log, how long it stays busy; `None` = never).
struct Prep {
    token: String,
    log: std::path::PathBuf,
    busy: Option<Duration>,
    since: Mutex<Option<Instant>>,
}
static PREP: OnceLock<Prep> = OnceLock::new();

static STOP: AtomicBool = AtomicBool::new(false);

#[no_mangle]
pub extern "C" fn battalion_abi_version() -> u32 {
    ABI_VERSION
}

#[no_mangle]
pub extern "C" fn battalion_app_stop() {
    if !HANG.load(Ordering::SeqCst) {
        STOP.store(true, Ordering::SeqCst);
    }
}
static HANG: AtomicBool = AtomicBool::new(false);

/// # Safety
/// `context` is a NUL-terminated UTF-8 JSON string, valid for the call.
#[no_mangle]
pub unsafe extern "C" fn battalion_app_run(context: *const c_char) -> i32 {
    let text = CStr::from_ptr(context).to_string_lossy().to_string();
    match serde_json::from_str::<Context>(&text) {
        Ok(ctx) => run(ctx),
        Err(e) => {
            eprintln!("demo-app: bad context: {e}");
            2
        }
    }
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|x| x == name).and_then(|i| args.get(i + 1).cloned())
}

fn run(ctx: Context) -> i32 {
    let mode = arg(&ctx.args, "--mode").or_else(|| std::env::var("DEMO_MODE").ok()).unwrap_or_else(|| "normal".into());
    let delay = |default: u64| Duration::from_millis(arg(&ctx.args, "--delay-ms").and_then(|v| v.parse().ok()).unwrap_or(default));
    let token = std::fs::read_to_string(&ctx.token_file).unwrap_or_default().trim().to_string();
    let ready_file = ctx.ready_file.clone();
    let prepare = matches!(mode.as_str(), "prepare" | "prepare-never");
    if prepare {
        let _ = std::fs::create_dir_all(&ctx.state_dir);
        let busy = (mode == "prepare").then(|| delay(1500));
        let _ = PREP.set(Prep { token: token.clone(), log: std::path::Path::new(&ctx.state_dir).join("prepare.log"), busy, since: Mutex::new(None) });
    }

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    listener.set_nonblocking(true).expect("nonblocking");
    let port = listener.local_addr().unwrap().port();
    let counter = Arc::new(AtomicU64::new(0));
    {
        let c = counter.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(1));
            c.fetch_add(1, Ordering::Relaxed);
        });
    }

    let ready = |contract: u32| {
        let r = ReadyFile { contract, pid: std::process::id(), url: format!("http://127.0.0.1:{port}"), health: "/health".into(), stop: "/stop".into(), prepare: prepare.then(|| "/prepare".into()) };
        // Write then rename: the OS never reads a half-written ready file.
        let tmp = format!("{ready_file}.tmp");
        std::fs::write(&tmp, serde_json::to_vec(&r).unwrap()).expect("ready tmp");
        std::fs::rename(&tmp, &ready_file).expect("ready file");
    };

    match mode.as_str() {
        "never-ready" => {
            while !STOP.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(20));
            }
            return 0;
        }
        "wrong-contract" => ready(ctx.contract + 1),
        "slow" => {
            std::thread::sleep(delay(2000));
            ready(ctx.contract)
        }
        "crash" | "abort" => ready(ctx.contract),
        "hang" => {
            HANG.store(true, Ordering::SeqCst);
            ready(ctx.contract)
        }
        "child" => {
            let helper = std::path::Path::new(&ctx.app_dir).join("bin").join(if cfg!(windows) { "demo-helper.exe" } else { "demo-helper" });
            let mut c = std::process::Command::new(helper);
            c.args(["--sleep", "120"]).stdin(std::process::Stdio::null());
            let child = c.spawn().expect("start the helper");
            let _ = std::fs::create_dir_all(&ctx.state_dir);
            let _ = std::fs::write(std::path::Path::new(&ctx.state_dir).join("child.pid"), child.id().to_string());
            std::mem::forget(child);
            ready(ctx.contract)
        }
        "env" => {
            let _ = std::fs::create_dir_all(&ctx.state_dir);
            let text: String = std::env::vars().map(|(k, v)| format!("{k}={v}
")).collect();
            let _ = std::fs::write(std::path::Path::new(&ctx.state_dir).join("env.txt"), text);
            ready(ctx.contract)
        }
        "normal" | "prepare" | "prepare-never" => ready(ctx.contract),
        other => {
            eprintln!("demo-app: unknown mode '{other}'");
            return 2;
        }
    }

    let end_at = matches!(mode.as_str(), "crash" | "abort").then(|| std::time::Instant::now() + delay(500));
    while !STOP.load(Ordering::SeqCst) {
        if end_at.is_some_and(|t| std::time::Instant::now() >= t) {
            if mode == "abort" {
                std::process::abort();
            }
            return 1; // `run` returns an error code: the app ended
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if mode == "hang" {
                    // Accept, say nothing, keep the socket open: a wedged app.
                    std::mem::forget(stream);
                    continue;
                }
                let counter = counter.clone();
                let ui_origin = ctx.ui_origin.clone();
                std::thread::spawn(move || serve(stream, &counter, ui_origin.as_deref()));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    0
}

/// `GET /ws` stands for an app's socket: it lets exactly the origin the OS named (`uiOrigin`) in (101) and refuses every other
/// page (403). `GET /health` and the rest answer as before.
fn serve(mut stream: TcpStream, counter: &AtomicU64, ui_origin: Option<&str>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut origin: Option<String> = None;
    let (mut bearer, mut length) = (None::<String>, 0usize);
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.eq_ignore_ascii_case("origin") {
                origin = Some(v.trim().to_string());
            } else if k.eq_ignore_ascii_case("authorization") {
                bearer = v.trim().strip_prefix("Bearer ").map(str::to_string);
            } else if k.eq_ignore_ascii_case("content-length") {
                length = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let reply = |stream: &mut TcpStream, status: &str, body: &str| {
        let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    };
    match request_line.split_whitespace().take(2).collect::<Vec<_>>().as_slice() {
        ["GET", "/health"] => {
            let h = Health { ok: true, busy: Some(if PREP.get().is_some() { 1 } else { 0 }), detail: Some(format!("counter={}", counter.load(Ordering::Relaxed))) };
            reply(&mut stream, "200 OK", &serde_json::to_string(&h).unwrap());
        }
        ["POST", "/prepare"] if PREP.get().is_some() => {
            let p = PREP.get().unwrap();
            let mut body = vec![0u8; length.min(64 * 1024)];
            let _ = std::io::Read::read_exact(&mut reader, &mut body);
            let note = |line: &str| {
                use std::io::Write as _;
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p.log) {
                    let _ = writeln!(f, "{line}");
                }
            };
            if bearer.as_deref() != Some(p.token.as_str()) {
                note("refused");
                return reply(&mut stream, "401 Unauthorized", "{\"ok\":false}");
            }
            let req: battalion_app_api::PrepareRequest = serde_json::from_slice(&body).unwrap_or_default();
            let answer = |ready: bool, eta: Option<u64>| serde_json::to_string(&battalion_app_api::PrepareAnswer { ready, eta_ms: eta, reason: (!ready).then(|| "its demo work finishes".into()) }).unwrap();
            if req.cancel {
                *p.since.lock().unwrap() = None;
                note("cancel");
                return reply(&mut stream, "200 OK", &answer(false, None));
            }
            if req.now {
                note("now");
                return reply(&mut stream, "200 OK", &answer(true, None));
            }
            let since = *p.since.lock().unwrap().get_or_insert_with(Instant::now);
            match p.busy.map(|b| b.saturating_sub(since.elapsed())) {
                Some(left) if left.is_zero() => {
                    note("ready");
                    reply(&mut stream, "200 OK", &answer(true, None))
                }
                left => {
                    note("wait");
                    reply(&mut stream, "200 OK", &answer(false, Some(left.map(|l| l.as_millis() as u64).unwrap_or(60_000))))
                }
            }
        }
        // What the demo page asks over fetch: allowed (with CORS for exactly that origin) only from the origin the OS named.
        ["GET", "/api/origin"] => match (origin.as_deref(), ui_origin) {
            (Some(o), Some(u)) if o == u => {
                let body = "{\"ok\":true}";
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: {u}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
            _ => reply(&mut stream, "403 Forbidden", &format!("{{\"expected\":{:?},\"got\":{:?}}}", ui_origin, origin)),
        },
        // The app's own script for its page (code the app serves to its own page): the page server's policy lets the page load it.
        ["GET", "/extra.js"] => {
            let body = "window.__demoExtra = 'loaded from the app';";
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
        ["GET", "/ws"] => match (origin.as_deref(), ui_origin) {
            (Some(o), Some(u)) if o == u => {
                let _ = write!(stream, "HTTP/1.1 101 Switching Protocols\r\nConnection: close\r\n\r\n");
            }
            _ => reply(&mut stream, "403 Forbidden", "{\"ok\":false}"),
        },
        _ => reply(&mut stream, "404 Not Found", "{\"ok\":false}"),
    }
}

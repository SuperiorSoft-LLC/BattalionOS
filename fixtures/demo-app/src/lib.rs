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
use std::sync::Arc;
use std::time::Duration;

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
    let _ = token;
    let ready_file = ctx.ready_file.clone();

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
        let r = ReadyFile { contract, pid: std::process::id(), url: format!("http://127.0.0.1:{port}"), health: "/health".into(), stop: "/stop".into() };
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
        "normal" => ready(ctx.contract),
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
                std::thread::spawn(move || serve(stream, &counter));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    0
}

fn serve(mut stream: TcpStream, counter: &AtomicU64) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
            break;
        }
    }
    let reply = |stream: &mut TcpStream, status: &str, body: &str| {
        let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    };
    match request_line.split_whitespace().take(2).collect::<Vec<_>>().as_slice() {
        ["GET", "/health"] => {
            let h = Health { ok: true, busy: Some(0), detail: Some(format!("counter={}", counter.load(Ordering::Relaxed))) };
            reply(&mut stream, "200 OK", &serde_json::to_string(&h).unwrap());
        }
        _ => reply(&mut stream, "404 Not Found", "{\"ok\":false}"),
    }
}

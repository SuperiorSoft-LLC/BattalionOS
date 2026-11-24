//! Fixture app for contract v1. Run it the way the OS does: with the BATTALION_* environment.
//!
//! Modes (`--mode <m>` or env `DEMO_MODE`):
//!   normal          ready at once; answers health and stop
//!   slow            writes the ready file after `--delay-ms` (default 2000)
//!   hang            writes the ready file, then never answers health or stop
//!   crash           writes the ready file, then exits with code 1 after `--delay-ms` (default 500)
//!   never-ready     runs, never writes the ready file
//!   wrong-contract  writes a ready file that says contract 2
//! `--init` runs the init part: creates `init.done` in the state dir and exits 0.
//!
//! Health detail is `counter=<n>`: it grows once a second, so a test can see that another app
//! kept running (same pid, counter still growing) while this one was killed, hung or updated.

use battalion_app_api::{env, Health, ReadyFile};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn arg(name: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter().position(|x| x == name).and_then(|i| a.get(i + 1).cloned())
}

fn need(var: &str) -> String {
    std::env::var(var).unwrap_or_else(|_| {
        eprintln!("demo-app: {var} is not set (run it through the OS)");
        std::process::exit(2)
    })
}

fn main() {
    if std::env::args().any(|a| a == "--init") {
        let state = need(env::STATE_DIR);
        std::fs::create_dir_all(&state).expect("state dir");
        std::fs::write(std::path::Path::new(&state).join("init.done"), b"ok").expect("init file");
        return;
    }
    let mode = arg("--mode").or_else(|| std::env::var("DEMO_MODE").ok()).unwrap_or_else(|| "normal".into());
    let delay = |default: u64| Duration::from_millis(arg("--delay-ms").and_then(|v| v.parse().ok()).unwrap_or(default));
    let ready_file = need(env::READY_FILE);
    let token = std::fs::read_to_string(need(env::TOKEN_FILE)).unwrap_or_default().trim().to_string();
    let expected = need(env::CONTRACT).parse::<u32>().unwrap_or(1);

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
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
        "never-ready" => loop {
            std::thread::sleep(Duration::from_secs(3600));
        },
        "wrong-contract" => ready(expected + 1),
        "slow" => {
            std::thread::sleep(delay(2000));
            ready(expected)
        }
        "crash" => {
            ready(expected);
            std::thread::sleep(delay(500));
            std::process::exit(1)
        }
        "hang" | "normal" => ready(expected),
        other => {
            eprintln!("demo-app: unknown mode '{other}'");
            std::process::exit(2)
        }
    }

    for stream in listener.incoming().flatten() {
        if mode == "hang" {
            // Accept, say nothing, keep the socket open: a wedged app.
            std::mem::forget(stream);
            continue;
        }
        let (counter, token) = (counter.clone(), token.clone());
        std::thread::spawn(move || {
            if serve(stream, &counter, &token) {
                std::process::exit(0);
            }
        });
    }
}

/// Returns true when the app was told to stop.
fn serve(mut stream: TcpStream, counter: &AtomicU64, token: &str) -> bool {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return false;
    }
    let (mut authorized, mut len) = (false, 0usize);
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
            break;
        }
        let lower = h.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("authorization:") {
            authorized = !token.is_empty() && v.trim() == format!("bearer {}", token.to_ascii_lowercase());
        }
        if let Some(v) = lower.strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; len.min(4096)];
    let _ = reader.read_exact(&mut body);

    let reply = |stream: &mut TcpStream, status: &str, body: &str| {
        let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    };
    match request_line.split_whitespace().take(2).collect::<Vec<_>>().as_slice() {
        ["GET", "/health"] => {
            let h = Health { ok: true, busy: Some(0), detail: Some(format!("counter={}", counter.load(Ordering::Relaxed))) };
            reply(&mut stream, "200 OK", &serde_json::to_string(&h).unwrap());
            false
        }
        ["POST", "/stop"] if authorized => {
            reply(&mut stream, "200 OK", "{\"ok\":true}");
            true
        }
        ["POST", "/stop"] => {
            reply(&mut stream, "401 Unauthorized", "{\"ok\":false}");
            false
        }
        _ => {
            reply(&mut stream, "404 Not Found", "{\"ok\":false}");
            false
        }
    }
}

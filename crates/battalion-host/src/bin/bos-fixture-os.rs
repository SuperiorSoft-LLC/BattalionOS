// SPDX-License-Identifier: MPL-2.0
//! Test fixture: a tiny stand-in for the BattalionOS program. It answers to the worker argument
//! exactly like the real OS (`battalion_host::worker::run_if_worker`) and can run a supervisor, so
//! tests can kill "the OS" by pid and see what happens to its apps. Not part of any package.
//!
//!   bos-fixture-os serve <home> <app-id>...   starts the apps, prints `UP <id> <pid>` per app, then
//!                                             reads stdin: `exit` stops the apps cleanly and ends;
//!                                             end of file just ends the process (like a crash).

use battalion_host::supervisor::{Settings, Supervisor};
use battalion_host::Paths;
use std::io::BufRead;

fn main() {
    if let Some(code) = battalion_host::worker::run_if_worker() {
        std::process::exit(code);
    }
    let a: Vec<String> = std::env::args().collect();
    if a.get(1).map(String::as_str) != Some("serve") || a.len() < 4 {
        eprintln!("bos-fixture-os serve <home> <app-id>...");
        std::process::exit(2);
    }
    let paths = Paths::new(&a[2]);
    let sup = Supervisor::with_worker_exe(paths, Settings::default(), std::env::current_exe().unwrap());
    for id in &a[3..] {
        match sup.start(id) {
            Ok(s) => println!("UP {id} {}", s.pid.unwrap_or(0)),
            Err(e) => println!("FAIL {id} {e}"),
        }
    }
    println!("READY {}", std::process::id());
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        if line.trim() == "exit" {
            for (id, e) in sup.stop_all() {
                println!("STOPFAIL {id} {e}");
            }
            println!("EXITED");
            return;
        }
    }
    std::process::exit(0);
}

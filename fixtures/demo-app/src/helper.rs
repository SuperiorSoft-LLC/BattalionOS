// SPDX-License-Identifier: Apache-2.0
//! Helper program of the demo package. The app itself runs it; the OS never starts it.
//!   --init           creates `init.done` in the state dir (BATTALION_STATE_DIR) and exits 0
//!   --sleep <secs>   sleeps (a child of the app, to see that it ends with the app's group)

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.iter().any(|x| x == "--init") {
        let state = std::env::var("BATTALION_STATE_DIR").unwrap_or_else(|_| {
            eprintln!("demo-helper: BATTALION_STATE_DIR is not set");
            std::process::exit(2)
        });
        std::fs::create_dir_all(&state).expect("state dir");
        std::fs::write(std::path::Path::new(&state).join("init.done"), b"ok").expect("init file");
        return;
    }
    if let Some(i) = a.iter().position(|x| x == "--sleep") {
        let secs: u64 = a.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(60);
        std::thread::sleep(std::time::Duration::from_secs(secs));
        return;
    }
    eprintln!("demo-helper: --init or --sleep <secs>");
    std::process::exit(2)
}

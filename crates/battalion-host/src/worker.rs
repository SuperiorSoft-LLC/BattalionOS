// SPDX-License-Identifier: MPL-2.0
//! The app worker: the OS's own program started with an internal argument. It loads one app's
//! library, calls `run` on the app's own thread and `stop` when the OS closes its stdin (which also
//! happens when the OS is gone). It refuses to run when BattalionOS did not start it.
//!
//! Handshake (all before any library is touched, so the OS has already put the worker in its
//! Job Object): env `BATTALION_WORKER_NONCE` = a random value the OS made for this start, and
//! stdin line 1 = the same value, line 2 = the app's context JSON (`battalion_app_api::abi::Context`).
//! Started by hand there is no nonce: the worker says so and ends with code 2.

use battalion_app_api::abi::{ABI_VERSION, SYM_RUN, SYM_STOP, SYM_VERSION};
use std::ffi::CString;
use std::io::{BufRead, Read};
use std::sync::mpsc;

/// The internal argument: `<os> --bos-app-worker <library>`.
pub const ARG: &str = "--bos-app-worker";
pub const NONCE_ENV: &str = "BATTALION_WORKER_NONCE";
/// Exit code of a worker that refused to run or could not load the library.
pub const REFUSED: i32 = 2;

type RunFn = unsafe extern "C" fn(*const std::os::raw::c_char) -> i32;
type StopFn = unsafe extern "C" fn();

/// First thing in the OS's `main`, before any window code. `Some(code)` = this process was a
/// worker and it has ended: exit with the code. `None` = a normal start.
pub fn run_if_worker() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some(ARG) {
        return None;
    }
    Some(match worker_main(args.get(2).map(String::as_str)) {
        Ok(code) => code,
        Err(m) => {
            eprintln!("BattalionOS app worker: {m}");
            REFUSED
        }
    })
}

enum Event {
    Stop,
    Done(i32),
}

fn worker_main(library: Option<&str>) -> Result<i32, String> {
    let nonce = std::env::var(NONCE_ENV).map_err(|_| "this program is started by BattalionOS only; refusing to run".to_string())?;
    std::env::remove_var(NONCE_ENV);
    let library = library.ok_or("no library given")?.to_string();
    let stdin = std::io::stdin();
    let mut lock = stdin.lock();
    let mut line = String::new();
    lock.read_line(&mut line).map_err(|e| format!("no handshake: {e}"))?;
    if nonce.is_empty() || line.trim_end() != nonce {
        return Err("the handshake does not match; refusing to run".into());
    }
    let mut ctx = String::new();
    lock.read_line(&mut ctx).map_err(|e| format!("no context: {e}"))?;
    let ctx = CString::new(ctx.trim_end()).map_err(|_| "context has a NUL".to_string())?;
    drop(lock);

    // SAFETY: loading an app's library is the point of the worker; the library is the app's code.
    let lib = unsafe { libloading::Library::new(&library) }.map_err(|e| format!("cannot load {library}: {e}"))?;
    let (run, stop): (RunFn, StopFn) = unsafe {
        let version: libloading::Symbol<unsafe extern "C" fn() -> u32> = lib.get(SYM_VERSION).map_err(|e| format!("{library} has no battalion_abi_version: {e}"))?;
        let v = version();
        if v != ABI_VERSION {
            return Err(format!("{library} speaks app ABI {v}, this BattalionOS speaks {ABI_VERSION}"));
        }
        let run: libloading::Symbol<RunFn> = lib.get(SYM_RUN).map_err(|e| format!("{library} has no battalion_app_run: {e}"))?;
        let stop: libloading::Symbol<StopFn> = lib.get(SYM_STOP).map_err(|e| format!("{library} has no battalion_app_stop: {e}"))?;
        (*run, *stop)
    };
    std::mem::forget(lib); // loaded until the process ends

    let (tx, rx) = mpsc::channel::<Event>();
    // The OS holds our stdin: end of file = stop (also when the OS itself is gone).
    let tx_in = tx.clone();
    std::thread::Builder::new()
        .name("bos-worker-stdin".into())
        .spawn(move || {
            let mut sink = [0u8; 256];
            let mut s = std::io::stdin();
            while matches!(s.read(&mut sink), Ok(n) if n > 0) {}
            let _ = tx_in.send(Event::Stop);
        })
        .map_err(|e| e.to_string())?;
    // The app's own thread.
    std::thread::Builder::new()
        .name("bos-app".into())
        .spawn(move || {
            let code = std::panic::catch_unwind(|| unsafe { run(ctx.as_ptr()) }).unwrap_or(101);
            let _ = tx.send(Event::Done(code));
        })
        .map_err(|e| e.to_string())?;
    let mut stopped = false;
    loop {
        match rx.recv() {
            Ok(Event::Done(code)) => return Ok(code),
            Ok(Event::Stop) if !stopped => {
                stopped = true;
                let _ = std::panic::catch_unwind(|| unsafe { stop() });
            }
            Ok(Event::Stop) => {}
            Err(_) => return Ok(0),
        }
    }
}

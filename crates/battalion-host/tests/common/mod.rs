// SPDX-License-Identifier: MPL-2.0
//! Shared by the host tests: the demo library and its helper program, and a supervisor whose
//! workers are the fixture OS program (the test binary itself cannot be a worker).
#![allow(dead_code)]

use battalion_host::supervisor::{Settings, Supervisor};
use battalion_host::Paths;
use std::path::{Path, PathBuf};

/// The fixture OS program: answers to the worker argument like the real OS.
pub fn fixture_os() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_bos-fixture-os"))
}

pub fn sup(paths: Paths, settings: Settings) -> Supervisor {
    Supervisor::with_worker_exe(paths, settings, fixture_os())
}

/// The fixture's build output, freshly built: a cdylib is not a dependency of the tests, so cargo
/// would not rebuild a stale one. `cargo build -p demo-app` is a no-op when it is current.
fn find(name: &str, build: &[&str]) -> PathBuf {
    static BUILT: std::sync::Once = std::sync::Once::new();
    BUILT.call_once(|| {
        let ws = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert!(std::process::Command::new("cargo").args(build).current_dir(ws).status().unwrap().success(), "could not build the fixture");
    });
    std::env::current_exe().unwrap().ancestors().skip(1).take(6).map(|d| d.join(name)).find(|p| p.is_file()).unwrap_or_else(|| panic!("{name} not found near the test binary"))
}

/// File name of the demo library on this platform and its place inside a package.
pub const LIB_IN_PACKAGE: &str = if cfg!(windows) {
    "lib/demo_app.dll"
} else if cfg!(target_os = "macos") {
    "lib/libdemo_app.dylib"
} else {
    "lib/libdemo_app.so"
};
pub const HELPER_IN_PACKAGE: &str = if cfg!(windows) { "bin/demo-helper.exe" } else { "bin/demo-helper" };

pub fn demo_lib() -> PathBuf {
    static P: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    P.get_or_init(|| {
        let name = if cfg!(windows) {
            "demo_app.dll"
        } else if cfg!(target_os = "macos") {
            "libdemo_app.dylib"
        } else {
            "libdemo_app.so"
        };
        find(name, &["build", "-p", "demo-app"])
    })
    .clone()
}

pub fn demo_helper() -> PathBuf {
    static P: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    P.get_or_init(|| find(if cfg!(windows) { "demo-helper.exe" } else { "demo-helper" }, &["build", "-p", "demo-app"])).clone()
}

/// Lay the demo package into a version folder: the library, the helper program, `app.json`.
pub fn lay_demo(vdir: &Path) {
    std::fs::create_dir_all(vdir.join("lib")).unwrap();
    std::fs::create_dir_all(vdir.join("bin")).unwrap();
    std::fs::copy(demo_lib(), vdir.join(LIB_IN_PACKAGE)).unwrap();
    std::fs::copy(demo_helper(), vdir.join(HELPER_IN_PACKAGE)).unwrap();
}

pub fn pid_alive(pid: u32) -> bool {
    battalion_host::process::pid_running(pid)
}

/// End a process by pid, tree and all (a test cleanup; never by name).
pub fn kill_pid_tree(pid: u32) {
    battalion_host::process::kill_tree(&battalion_host::process::ProcId { pid, start: String::new() });
}

/// Whether the process is inside any Job Object (Windows) or leads its own group (unix).
pub fn in_job(pid: u32) -> bool {
    #[cfg(windows)]
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn OpenProcess(a: u32, i: i32, p: u32) -> *mut std::ffi::c_void;
            fn CloseHandle(h: *mut std::ffi::c_void) -> i32;
            fn IsProcessInJob(p: *mut std::ffi::c_void, job: *mut std::ffi::c_void, r: *mut i32) -> i32;
        }
        let h = OpenProcess(0x1000, 0, pid);
        assert!(!h.is_null());
        let mut r = 0;
        let ok = IsProcessInJob(h, std::ptr::null_mut(), &mut r);
        CloseHandle(h);
        ok != 0 && r != 0
    }
    #[cfg(unix)]
    {
        let out = std::process::Command::new("ps").args(["-o", "pgid=", "-p", &pid.to_string()]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim() == pid.to_string()
    }
}

/// The parent of a process (unix: ps; windows: toolhelp snapshot).
pub fn parent_of(pid: u32) -> Option<u32> {
    #[cfg(windows)]
    unsafe {
        #[repr(C)]
        struct Entry {
            size: u32,
            usage: u32,
            pid: u32,
            heap: usize,
            module: u32,
            threads: u32,
            parent: u32,
            pri: i32,
            flags: u32,
            exe: [u16; 260],
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn CreateToolhelp32Snapshot(f: u32, p: u32) -> *mut std::ffi::c_void;
            fn Process32FirstW(s: *mut std::ffi::c_void, e: *mut Entry) -> i32;
            fn Process32NextW(s: *mut std::ffi::c_void, e: *mut Entry) -> i32;
            fn CloseHandle(h: *mut std::ffi::c_void) -> i32;
        }
        let snap = CreateToolhelp32Snapshot(2, 0);
        let mut e: Entry = std::mem::zeroed();
        e.size = std::mem::size_of::<Entry>() as u32;
        let mut found = None;
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            if e.pid == pid {
                found = Some(e.parent);
                break;
            }
            ok = Process32NextW(snap, &mut e);
        }
        CloseHandle(snap);
        found
    }
    #[cfg(unix)]
    {
        let out = std::process::Command::new("ps").args(["-o", "ppid=", "-p", &pid.to_string()]).output().ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }
}

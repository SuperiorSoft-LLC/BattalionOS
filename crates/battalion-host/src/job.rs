// SPDX-License-Identifier: MPL-2.0
//! One group per app worker: on Windows a Job Object with KILL_ON_JOB_CLOSE (the worker is in it from
//! its first instruction, every process it starts joins it, and when the OS holding the handle ends
//! for any reason the kernel ends the whole group); on unix the worker's process group.
//!
//! Ported from the runner crate's `job.rs` in the agenthq-standalone repo (`rust/crates/*-runner/src/job.rs`,
//! same author, same idea; the OS names no app, so the crate's name is not written here). That file
//! uses the `windows-sys` crate; this one declares the few kernel32 calls itself so the OS crate
//! takes no new dependency for it.

use std::io;

pub struct Job {
    #[cfg(windows)]
    handle: *mut std::ffi::c_void,
    #[cfg(unix)]
    pgid: std::sync::atomic::AtomicI32,
}

// SAFETY: a job handle is a kernel handle, usable from any thread.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;
    #[repr(C)]
    pub struct BasicLimit {
        pub per_process_user_time: i64,
        pub per_job_user_time: i64,
        pub limit_flags: u32,
        pub min_ws: usize,
        pub max_ws: usize,
        pub active_process_limit: u32,
        pub affinity: usize,
        pub priority_class: u32,
        pub scheduling_class: u32,
    }
    #[repr(C)]
    pub struct IoCounters {
        pub v: [u64; 6],
    }
    #[repr(C)]
    pub struct ExtendedLimit {
        pub basic: BasicLimit,
        pub io: IoCounters,
        pub process_memory_limit: usize,
        pub job_memory_limit: usize,
        pub peak_process_memory: usize,
        pub peak_job_memory: usize,
    }
    pub const KILL_ON_JOB_CLOSE: u32 = 0x2000;
    pub const EXTENDED_LIMIT_INFO: i32 = 9;
    pub const SET_QUOTA: u32 = 0x0100;
    pub const TERMINATE: u32 = 0x0001;
    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateJobObjectW(attrs: *const c_void, name: *const u16) -> *mut c_void;
        pub fn SetInformationJobObject(job: *mut c_void, class: i32, info: *const c_void, len: u32) -> i32;
        pub fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
        pub fn TerminateJobObject(job: *mut c_void, code: u32) -> i32;
        pub fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        pub fn CloseHandle(h: *mut c_void) -> i32;
    }
}

#[cfg(windows)]
impl Job {
    pub fn new() -> io::Result<Job> {
        use sys::*;
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut info: ExtendedLimit = std::mem::zeroed();
            info.basic.limit_flags = KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(handle, EXTENDED_LIMIT_INFO, &info as *const _ as *const _, std::mem::size_of::<ExtendedLimit>() as u32);
            if ok == 0 {
                let e = io::Error::last_os_error();
                CloseHandle(handle);
                return Err(e);
            }
            Ok(Job { handle })
        }
    }
    /// Puts process `pid` (and every process it starts from now on) into the job.
    pub fn assign(&self, pid: u32) -> io::Result<()> {
        use sys::*;
        unsafe {
            let p = OpenProcess(SET_QUOTA | TERMINATE, 0, pid);
            if p.is_null() {
                return Err(io::Error::last_os_error());
            }
            let ok = AssignProcessToJobObject(self.handle, p);
            let e = io::Error::last_os_error();
            CloseHandle(p);
            if ok == 0 {
                Err(e)
            } else {
                Ok(())
            }
        }
    }
    /// Ends every process of the job now.
    pub fn kill(&self) {
        unsafe {
            sys::TerminateJobObject(self.handle, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            sys::CloseHandle(self.handle); // KILL_ON_JOB_CLOSE: the group ends with the handle
        }
    }
}

#[cfg(unix)]
extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(unix)]
impl Job {
    pub fn new() -> io::Result<Job> {
        Ok(Job { pgid: std::sync::atomic::AtomicI32::new(0) })
    }
    /// The worker leads its own process group (see `process::spawn_worker`): that group is the job.
    pub fn assign(&self, pid: u32) -> io::Result<()> {
        self.pgid.store(pid as i32, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    pub fn kill(&self) {
        let g = self.pgid.load(std::sync::atomic::Ordering::SeqCst);
        if g > 0 {
            unsafe {
                kill(-g, 9);
            }
        }
    }
}

#[cfg(unix)]
impl Drop for Job {
    fn drop(&mut self) {
        self.kill();
    }
}

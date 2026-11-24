// SPDX-License-Identifier: Apache-2.0
//! The C ABI between BattalionOS and an app's library (version 1).
//!
//! An app is a library, not a program. BattalionOS starts its own worker for the app (the OS's own
//! program, with an internal argument), the worker loads the library and calls these symbols:
//!
//! ```c
//! uint32_t battalion_abi_version(void);               /* must return 1 */
//! int32_t  battalion_app_run(const char *context_json); /* blocks until stopped; returns a code */
//! void     battalion_app_stop(void);                  /* asks run to return; may be called from another thread */
//! ```
//!
//! * `run` is called once, on the app's own thread, with a NUL-terminated UTF-8 JSON [`Context`]
//!   (valid only during the call; copy what you keep). It returns when the app has ended; the
//!   worker then exits with that code. `run` returning on its own is an app that ended (the OS
//!   applies its restart rules).
//! * `stop` is called from the worker's main thread when the OS asks the app to end (the OS closed
//!   the worker's stdin, which also happens when the OS itself is gone). The app should make `run`
//!   return promptly; if it does not within `stopTimeoutMs`, the OS ends the worker.
//! * The library is loaded once per worker; the worker is one app only, so statics are the app's own.
//! * The ready file, health and the loopback address stay as in contract v1 ([`crate::runtime`]).
//!   The environment of contract v1 is set in the worker's process before the library is loaded.
//! * A function that panics must not unwind across the ABI: catch it and return a non-zero code.
//!
//! Helper programs inside the package that the app itself runs (a CLI) are allowed. BattalionOS
//! never starts them.

use serde::{Deserialize, Serialize};

pub const ABI_VERSION: u32 = 1;
pub const SYM_VERSION: &[u8] = b"battalion_abi_version\0";
pub const SYM_RUN: &[u8] = b"battalion_app_run\0";
pub const SYM_STOP: &[u8] = b"battalion_app_stop\0";

/// The JSON handed to `run`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Context {
    pub contract: u32,
    pub abi: u32,
    #[serde(rename = "appId")]
    pub app_id: String,
    #[serde(rename = "appVersion")]
    pub app_version: String,
    /// The package folder of this version (where the library lives).
    #[serde(rename = "appDir")]
    pub app_dir: String,
    #[serde(rename = "stateDir")]
    pub state_dir: String,
    #[serde(rename = "readyFile")]
    pub ready_file: String,
    #[serde(rename = "tokenFile")]
    pub token_file: String,
    /// `process.<target>.args` of the manifest.
    pub args: Vec<String>,
}

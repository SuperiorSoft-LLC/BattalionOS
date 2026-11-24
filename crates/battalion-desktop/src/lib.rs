// SPDX-License-Identifier: MPL-2.0
//! BattalionOS desktop host (MPL-2.0). The pure parts (window labels, who may act on whom, the
//! per-app file server) need no GUI and are tested without one; `gui` adds the Tauri 2 host.

pub mod access;
pub mod badge;
pub mod events;
pub mod serve;
pub mod webview2;

#[cfg(feature = "gui")]
pub mod commands;
#[cfg(feature = "gui")]
pub mod host;

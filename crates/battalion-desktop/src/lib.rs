// SPDX-License-Identifier: MPL-2.0
//! BattalionOS desktop host (MPL-2.0). The pure parts (window labels, who may act on whom, the
//! per-app file server) need no GUI and are tested without one; `gui` adds the Tauri 2 host.

pub mod access;
pub mod badge;
pub mod events;
pub mod frame_calls;
pub mod launch;
pub mod osupdate;
pub mod serve;
#[cfg(feature = "gui")]
pub mod shortcuts;
pub mod webview2;
pub mod win_calls;
pub mod windows;

#[cfg(feature = "gui")]
pub mod commands;
#[cfg(feature = "dev")]
pub mod devmode;
#[cfg(all(feature = "gui", not(target_os = "android")))]
pub mod front;
#[cfg(feature = "gui")]
pub mod host;

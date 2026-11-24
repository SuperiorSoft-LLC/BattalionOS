// SPDX-License-Identifier: MPL-2.0
//! Event names the host emits to windows. Frozen in docs/commands.md.

pub const APP_STATE: &str = "os://app-state";
pub const APP_WINDOW_CLOSED: &str = "os://app-window-closed";
pub const LOGIN_CHANGED: &str = "os://login-changed";
/// `{id, op, stage, bytes, total, percent}` while the Armory installs, updates, rolls back or removes an app.
pub const ARMORY_PROGRESS: &str = "os://armory-progress";

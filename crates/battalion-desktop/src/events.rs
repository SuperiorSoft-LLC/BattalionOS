// SPDX-License-Identifier: MPL-2.0
//! Event names the host emits to windows. Frozen in docs/commands.md.

pub const APP_STATE: &str = "os://app-state";
pub const APP_WINDOW_CLOSED: &str = "os://app-window-closed";
/// `{id, badge}` (`badge` a text or null) when an app sets or clears its badge.
pub const BADGE: &str = "os://badge";
pub const LOGIN_CHANGED: &str = "os://login-changed";
/// `{id, op, stage, bytes, total, percent}` while the Armory installs, updates, rolls back or removes an app.
pub const ARMORY_PROGRESS: &str = "os://armory-progress";
/// `{status, ids?, id?, error?}` while the first-run install of the apps the installer asked for runs. `status` is `start`
/// (`ids`), `installed` (`id`), `done` (`ids`), `offline` or `failed` (`id`, `error`; Retry is `os_first_run_retry`), `already-done` or `nothing`.
pub const FIRST_RUN: &str = "os://first-run";

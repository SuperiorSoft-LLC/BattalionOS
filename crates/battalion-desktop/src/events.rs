// SPDX-License-Identifier: MPL-2.0
//! Event names the host emits to windows. Frozen in docs/commands.md.

pub const APP_STATE: &str = "os://app-state";
/// The app id: an app is to be shown inside the OS window (`os_open_app_window`).
pub const APP_OPEN: &str = "os://app-open";
pub const APP_WINDOW_CLOSED: &str = "os://app-window-closed";
/// `{id, badge}` (`badge` a text or null) when an app sets or clears its badge.
pub const BADGE: &str = "os://badge";
pub const LOGIN_CHANGED: &str = "os://login-changed";
/// `{id, op, stage, bytes, total, percent}` while the Armory installs, updates, rolls back or removes an app.
pub const ARMORY_PROGRESS: &str = "os://armory-progress";
/// `{status, ids?, id?, error?}` while the first-run install of the apps the installer asked for runs. `status` is `start`
/// (`ids`), `installed` (`id`), `done` (`ids`), `offline` or `failed` (`id`, `error`; Retry is `os_first_run_retry`), `already-done` or `nothing`.
pub const FIRST_RUN: &str = "os://first-run";
/// `{id, url}` (BOS-25): the sign-in window of app `id` reached a web address. The shell tells that app's frame.
pub const FRAME_LOGIN_URL: &str = "os://frame-login-url";
/// `{id}` (BOS-25): the sign-in window of app `id` is gone.
pub const FRAME_LOGIN_CLOSED: &str = "os://frame-login-closed";
/// (BOS-19) A start from outside asked for app `id` (`--open <id>`): the shell asks `os_take_open_request` and opens it.
pub const OPEN_REQUEST: &str = "os://open-request";
/// (BOS-30) `{current, available: {version, notes} | null}`: a check of the OS's own feed finished (the Armory shows it).
pub const OS_UPDATE: &str = "os://os-update";

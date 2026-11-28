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
/// `{id, tag, route}` (BOS-NOTIFY-1): the person clicked a desktop notice of app `id`. The shell brings the app forward and tells
/// that app's frame `{bos: 1, event: "os_notify_click", tag, route}`.
pub const FRAME_NOTIFY_CLICK: &str = "os://frame-notify-click";
/// (BOS-19) A start from outside asked for app `id` (`--open <id>`): the shell asks `os_take_open_request` and opens it.
pub const OPEN_REQUEST: &str = "os://open-request";
/// (BOS-30) `{current, available: {version, notes} | null}`: a check of the OS's own feed finished (the Armory shows it).
pub const OS_UPDATE: &str = "os://os-update";

/// `{id, status, ms?, error?}` (BOS-31, developer builds only): a dev app started (`started`), restarted after its library changed
/// (`restarted`, `ms` from the change seen to ready) or did not start (`failed`, `error`; the watching goes on).
pub const DEV_APP: &str = "os://dev-app";
/// UPDATE-LIVE-1: an app was updated, `{id, from, to}`: the shell closes its view and opens it again on the new version.
pub const APP_UPDATED: &str = "os://app-updated";
/// OS-WINDOW-1 (WM-2): `{bos: 1, event: "os_win", id, wid, change, ...}` to the owning app's windows only (the shell, `app-<id>` and its
/// native `win-<id>-<n>`). `change` is `state` (`state`, `bounds`), `closed` or `focus` (`z`, `state`). The shell forwards it to that app's
/// frames without `id`.
pub const WIN: &str = "os://win";

# BattalionOS desktop: windows, commands, events (frozen v1)

Crate `battalion-desktop` (Tauri 2.12.6). Names below are frozen: later tickets may add, never rename.

## Windows

| Label | What | Webview data | Origin |
|---|---|---|---|
| `main` | The OS shell. Single instance: a second launch focuses it. | Tauri default | the bundled shell |
| `login` | Sign-in. Opened by `os_open_login`. | Tauri default | the bundled `login.html` |
| `app-<id>` | One app. `<id>` is a valid app id (`[a-z0-9-]{2,63}`). | `<root>/webview/<id>` (`Paths::webview_dir`) | `http://127.0.0.1:<port>` of that app's own server |

The window label is the caller's identity. It comes from the Tauri runtime, never from an argument.

## Origin (spike result)

An origin per app at run time on Windows: a custom scheme (`bos://`) is registered once, when the
Tauri builder is built, and WebView2 maps it to `http://bos.localhost` for every app. A scheme per app
would need every app id at build time, so it cannot serve apps installed later. **Used: the loopback
fallback.** Each open app window gets its own server on `127.0.0.1:<free port>`, so its origin is its own;
its `data_directory` separates cookies and storage. The server (`src/serve.rs`) serves only files under
the app's version folder, GET/HEAD, refuses `..`, `\`, `:`, NUL, double-encoding and anything whose real
path leaves the folder (symlink, junction), checks the `Host` header (rebinding), and sends a CSP whose
`connect-src` is `'self'` plus the manifest's `ui.connect`. Closing the window drops the server.

## Capabilities

`capabilities/main.json` (label `main`): every command. `capabilities/app.json` (label `app-*`, remote
URL `http://127.0.0.1:*/*`): `os_window_info`, `os_app_status`, `os_app_health`, `os_close_app_window`, `os_badge`, `os_show_desktop`.
`capabilities/login.json` (label `login`): `os_window_info`, `os_login_done`.
Every command checks the label again in Rust: an `app-a` window cannot act on app `b`.

## Commands (all `async`; blocking work runs on a blocking thread; errors are strings)

| Command | Args | Who | Returns |
|---|---|---|---|
| `os_window_info` | | any | `{label, app}` (`app` is the id or null) |
| `os_list_apps` | | main | `[{id, version, state, channel, name, icon, available, previous}]`: `name` from the installed manifest; `icon` its icon file as a `data:` URL (null when there is none, it is over 256 KiB or it is not a png, svg, jpg, webp, gif or ico); `available` the newer version the last feed read offered, or null (no network at this call); `previous` the version a rollback goes back to, or null |
| `os_app_status` | `id` | main, `app-<id>` | `{id, state, pid, url, paused, restarts, lastError}` |
| `os_app_health` | `id` | main, `app-<id>` | health, as text |
| `os_start_app` | `id` | main | status |
| `os_stop_app` | `id` | main | status |
| `os_open_app_window` | `id` | main | nothing; opens or focuses `app-<id>` |
| `os_close_app_window` | `id` | main, `app-<id>` | nothing; the app process keeps running |
| `os_catalog` | | main | `{apps:[{id, source, compatible, name, publisher, summary, icon, version}], rejected, sourceErrors, fromCache}`: `icon` only when the catalog gives a `data:image/` or `https:` URL; `version` is what the app's stable feed offers (null when the feed cannot be read) |
| `os_install_app` | `id`, `channel?` (default `stable`) | main | `{ok}` |
| `os_open_login` | | main | nothing |
| `os_login_done` | | login | nothing; closes the login window, emits `os://login-changed` |

| `os_update_app` | `id` | main | `{id, from, to}`; refused: up to date, busy app, bad signature, other key. Stops and starts only this app |
| `os_rollback_app` | `id` | main | `{id, from, to}`; refused when there is no previous version or the state schema changed |
| `os_remove_app` | `id` | main | `{ok}`; closes the app's window, stops it, deletes its versions; its state folder stays |
| `os_exit` | | main | nothing; ends the OS process only (see Lifetime) |

Added by BOS-10b. Every command above runs the Armory action on a blocking thread and, when it ends
(done or failed), emits `os://app-state`. Errors are the Armory's own text, shown with a Retry.

Added by BOS-10c. Both are for an app window only (the `app-*` capability); `main` and `login` are refused. The app is the
window that calls (its label from the runtime); neither takes an id.

| Command | Args | Who | Returns |
|---|---|---|---|
| `os_badge` | `count?` (number), `text?` (string) | `app-<id>` | nothing. Sets the caller's own badge: a count (above 99 shows `99+`) or a text of up to 8 characters, not both; `count` 0, an empty text, or neither clears it. The desktop icon shows it (`badge` in `os_list_apps`), and so do the app window's taskbar button (an overlay dot on Windows, the badge label where the OS has one) and its title `(<badge>) <id>`. It is cleared when the app stops (`os_stop_app`), is updated, rolled back or removed, or is no longer running when `os_list_apps` is read |
| `os_show_desktop` | | `app-<id>` | nothing. Brings `main` to the front (shown, un-minimized, focused) and minimizes the calling app window; the window stays open and the app keeps running |

`os_list_apps` rows also carry `badge`: the running app's badge text, or null.

## Events (emitted to all windows)

| Event | Payload |
|---|---|
| `os://app-state` | `{id, state}` after `os_install_app`, `os_update_app`, `os_rollback_app` and `os_remove_app` end (`state` is null for a removed app) |
| `os://armory-progress` | `{id, op, stage, bytes, total, percent}`: `op` is `install`, `update`, `rollback` or `remove`; `stage` is the step (`download`, `verify`, `unpack`, `place`, `init`, `stop`, `switch`, `start`, `done`, `mark`, `delete`, `forget`); `bytes`, `total` and `percent` (0-100) only while `stage` is `download`, else null |
| `os://app-window-closed` | the app id |
| `os://login-changed` | `true` |
| `os://badge` | `{id, badge}` when an app sets or clears its badge (`badge` is the text or null) |

## Lifetime

An app runs on its own worker, owned by the OS (see `docs/app-abi.md`). Closing the OS window only hides it:
apps and agents keep working, and a new start of the OS shows the window again. An app's own window closing
leaves the app running. `os_exit` (the shell's Exit) is the only way the OS ends: the exit handler calls
`Armory::exit_os`, which stops every app cleanly (the app's `stop`, its `stopTimeoutMs`, then its job), and then
the OS ends. If the OS ends any other way (it is killed), every worker's job and stdin close with it and the apps
end too. Nothing is adopted at the next start; a leftover of an older run is stopped by its recorded pid and start
time only (`Supervisor::recover_all`), never by name.

Added by BOS-11 (first run). The installer lays `first-run.json` beside the executable only for the apps left checked; the OS
installs them in the background on open and writes `first-run.state.json` (the done marker) in the OS home once every one is in.

| Command | Args | Who | Returns |
|---|---|---|---|
| `os_first_run_retry` | | main | `{outcome}` (text); runs the first-run install again (Retry). Does nothing once the marker says done |

Event `os://first-run` `{status, ids?, id?, error?}`: `start` (ids), `installed` (id), `done` (ids), `offline` or `failed` (id, error), `already-done`, `nothing`.

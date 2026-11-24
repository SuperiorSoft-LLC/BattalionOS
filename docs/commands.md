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
URL `http://127.0.0.1:*/*`): `os_window_info`, `os_app_status`, `os_app_health`, `os_close_app_window`.
`capabilities/login.json` (label `login`): `os_window_info`, `os_login_done`.
Every command checks the label again in Rust: an `app-a` window cannot act on app `b`.

## Commands (all `async`; blocking work runs on a blocking thread; errors are strings)

| Command | Args | Who | Returns |
|---|---|---|---|
| `os_window_info` | | any | `{label, app}` (`app` is the id or null) |
| `os_list_apps` | | main | `[{id, version, state, channel}]` |
| `os_app_status` | `id` | main, `app-<id>` | `{id, state, pid, url, paused, restarts, lastError}` |
| `os_app_health` | `id` | main, `app-<id>` | health, as text |
| `os_start_app` | `id` | main | status |
| `os_stop_app` | `id` | main | status |
| `os_open_app_window` | `id` | main | nothing; opens or focuses `app-<id>` |
| `os_close_app_window` | `id` | main, `app-<id>` | nothing; the app process keeps running |
| `os_catalog` | | main | `{apps:[{id,source,compatible}], rejected, sourceErrors, fromCache}` |
| `os_install_app` | `id`, `channel?` (default `stable`) | main | `{ok}` |
| `os_open_login` | | main | nothing |
| `os_login_done` | | login | nothing; closes the login window, emits `os://login-changed` |

Not yet commands (a later ticket adds them under the same rules): update, rollback, remove.

## Events (emitted to all windows)

| Event | Payload |
|---|---|
| `os://app-state` | reserved: `{id, state}` when the supervisor reports a change |
| `os://app-window-closed` | the app id |
| `os://login-changed` | `true` |

## Lifetime

App exit = its window closes; the app's process keeps running under the supervisor (per manifest).
OS exit calls `Supervisor::release_all`: apps are let go, not stopped. At start the OS calls `adopt_all`.

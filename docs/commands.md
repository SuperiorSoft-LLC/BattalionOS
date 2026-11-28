# BattalionOS desktop: windows, commands, events (frozen v1)

Crate `battalion-desktop` (Tauri 2.12.6). Names below are frozen: later tickets may add, never rename.

## Windows

| Label | What | Webview data | Origin |
|---|---|---|---|
| `main` | The OS shell. Single instance: a second launch focuses it. | Tauri default | the bundled shell |
| `login` | Sign-in. Opened by `os_open_login`. | Tauri default | the bundled `login.html` |
| `app-<id>` | One app. `<id>` is a valid app id (`[a-z0-9-]{2,63}`). | `<root>/webview/<id>` (`Paths::webview_dir`) | `http://127.0.0.1:<port>` of that app's own server |

The window label is the caller's identity. It comes from the Tauri runtime, never from an argument.

## Windows (OS-WINDOW-1)

The OS's window model (WM-1) and its calls (WM-2). WM-3 adds the taskbar items, WM-6 native windows.
One `WindowManager` actor (`src/windows.rs`, a tokio task behind an mpsc) owns the window table; nothing else writes it.

| Field | What |
|---|---|
| `wid` | `w-<app>-<n>`, given by the OS. Scoped to its app: a call naming another app's wid is refused (`NotYours`). |
| `app` | The caller's identity (its window label or frame), never an argument. |
| `key` | Chosen by the app, stable. Opening an open `(app, key)` focuses it instead of a duplicate; it is the unit of persistence. |
| `kind` | `inline` \| `float` \| `native`. |
| `title`, `icon?`, `badge?` (`{count?, text?}`), `attention` | What the taskbar shows. |
| `state` | `normal` \| `minimized` \| `maximized` \| `{snap: left\|right\|tl\|tr\|bl\|br}`. |
| `bounds` | `{x, y, w, h}` in its surface's coordinates. |
| `z` | OS-owned stack order (higher is on top). |
| `route?` | `float`/`native` only: the app page path the frame loads. |

Persistence: `(app, key) -> {kind, bounds, state, monitor?}` in `<root>/state/windows/<app>.json`. Writes are debounced
by `bos.windows.saveDebounceMs` (os.json `{"windows": {"saveDebounceMs": 500}}`, default 500) and run on a blocking
thread. A minimized window keeps its last open state. Removing the app (`os_remove_app`) deletes the file; an update
keeps it. On restore the bounds are clamped to the current surface or monitor, so a window is never lost off-screen.

### Native windows (WM-6)

`os_win_open {kind: "native", route}` makes an OS window labelled `win-<app>-<n>` (the `<n>` of its wid `w-<app>-<n>`). It loads
the OS host page `window.html`: the `WindowFrame` bar (decorations off; the bar drags the window, so Windows snap works) and ONE
frame of the app at `route`, on the app's own origin, in the app's own webview data directory (the one its `app-<id>` window uses,
so it is not a second sign-in). The frame's bridge is the one of the frames in `main`; `os_window_info` there answers
`{label: "win-<app>-<n>", app, wid}`, and `os_app_status`/`os_app_health` answer for the window's own app (POPOUT-1: the page finds its daemon); the other native calls are refused there. `os_win_minimize`/`maximize`/`restore`/`focus`/`snap`/`close` act on the OS window;
`os_win_set {badge}` paints the overlay badge on its own taskbar button. The same key focuses the open window.

- Geometry is per monitor: the saved monitor while it is attached, else the primary; the bounds are clamped to its work area.
  Moving or resizing the window saves its bounds and monitor (debounced as above).
- The window closes when its app stops (`os_stop_app`); its geometry and route stay saved. Closing it from the OS (Alt+F4)
  takes it out of the table and the app hears `closed`.
- `bos.windows.restoreOnStart` (os.json `{"windows": {"restoreOnStart": true}}`, default false) reopens an app's native windows
  when the app starts.
- WM-7 low memory: a native window minimized or hidden for `bos.windows.lowMemoryAfterSec` seconds (os.json
  `{"windows": {"lowMemoryAfterSec": 30}}`, default 30, 0 = off) is set to WebView2's LOW `MemoryUsageTargetLevel`; restore or
  focus sets NORMAL again. `TrySuspend` is not used (it stops timers and sockets). Behind `NativeWindows::memory_level`
  (`None` on engines without one).
- `os_win_list` carries `memoryMb` on a native window when the runtime tells it (WebView2: the working set of the renderer
  processes holding that window's frames; a renderer shared with another window counts for both). The taskbar window list
  shows it.
- A phone has no windows of its own: `native` and `float` open as a slot view and `os_win_open` answers `kind: "inline"`.
- The engine is behind the `NativeWindows` trait (`battalion-desktop::native`); the Tauri/wry implementation (`native_tauri`) is
  the only one. Every window step runs from an async command through Tauri's main-thread event loop.

Geometry: `battalion-app-api::geometry` (Rust) and the kit's `lib/geometry.ts` (TypeScript) are the same pure
functions, shared by the OS and the SDKs: `clampBox`, `dragBox`, `resizeBox` (from infantry-ai `popout.ts`; minimum
320x240, 80 px kept grabbable, 36 px bar), `snapBox` (halves `left`/`right`, quarters `tl`/`tr`/`bl`/`br`) and
`snapZoneAt` (the zone a drag pointer near an edge or corner asks for).

### Window calls (WM-2)

An app's page calls these from its frame (the bridge message, `{bos: 1, id, cmd, args}`), or with `invoke` from its own `app-<id>`
window or a native `win-<app>-<n>` window. The app is the frame's or the label's, never an argument. Every call needs the `window`
permission in the app's installed `app.json` (read at every call); without it the call is refused with the permission's name. An app
names only its own wids (`access::authorize_wid`): a wid of another app is refused. There is no cap on the number of windows
(`bos.windows.max` is unset).

| Command | Args | Result |
|---|---|---|
| `os_win_open` | `key`, `kind?` (`inline` default), `title?`, `icon?`, `route?`, `bounds?`, `state?` | `{wid, kind, state, bounds, restored}`. A `key` already open: that window is focused and returned (`restored: false`). Saved state for `(app, key)` wins over `bounds`/`state` (`restored: true`). `route` must be a path on the app's own origin (`/...`, no scheme or host); it is kept for `float`/`native` only |
| `os_win_close` | `wid` | nothing. The window goes; its saved geometry is kept |
| `os_win_minimize` / `os_win_restore` / `os_win_maximize` | `wid` | `{state}` |
| `os_win_focus` | `wid` | `{state}`. Raises it to the top of its surface; a minimized window is restored first |
| `os_win_snap` | `wid`, `zone` (`left`, `right`, `tl`, `tr`, `bl`, `br`) | `{state, bounds}`: the half or quarter of the surface |
| `os_win_set` | `wid`, `title?`, `icon?`, `badge?` (`{count?, text?}`, `null` clears), `attention?`, `bounds?` | nothing. `bounds` only for `inline` (the app moved it; the OS saves it) |
| `os_win_list` | | the caller's own windows (`Window[]`, z order; native windows carry `memoryMb` where known) |

The SDK (WM-5) wraps these calls: Rust types in `battalion-app-api::windows` (`WindowSpec`, `WindowKind`, `WindowState`, `SnapZone`,
`WinEvent`; schema `schemas/window.schema.json`), and the TS `windows` client in the kit (`kit/lib/windows`, types generated from the
schema): `osBackend` over these calls, `localBackend` (standalone Tauri, browser) with the same rules in the page, `localStorage`, `inline`
only, drawn by `LocalDesk` with a dock strip as the taskbar.

The shell serves a frame's calls with main-only commands, one per call, the app id taken from the frame: `os_frame_win_open`,
`os_frame_win_close`, `os_frame_win_minimize`, `os_frame_win_restore`, `os_frame_win_maximize`, `os_frame_win_focus`,
`os_frame_win_snap`, `os_frame_win_set`, `os_frame_win_list`, each with `id` and `args` (the page's arguments; an `id` in them is dropped).

WM-8, the `float` kind: the shell draws a float with `WindowFrame` over the work area of the OS window (above app views, never under
the taskbar), with a frame of the app at `route` on the app's own origin (the same renderer as its view). The OS owns drag, resize,
snap, z-order, maximize (the work area), close and minimize (the window stays alive, hidden, listed under its app's taskbar button;
restore keeps its bounds). Floats close when their app stops. The frame in a float may call the window calls, `os_window_info`,
`os_app_status` and `os_app_health` for its own app only. Two main-only commands serve the chrome:

| Command | Args | Result |
|---|---|---|
| `os_shell_win_bounds` | `id`, `wid`, `bounds` | `{state, bounds}`: the OS moved or resized float `wid` (clamped to the work area, state `normal`). Refused for any other kind. |
| `os_shell_win_pop_out` | `id`, `wid` | The `os_win_open` answer of a native window with the float's key, route, title and size; the float closes first. |

A native window is a real OS window: it shows in the Windows taskbar and minimizes there; the BOS taskbar does not list it as minimized.

Event `os://win`, emitted only to the owning app's windows (the shell, `app-<id>`, `win-<id>-<n>`): `{bos: 1, event: "os_win", id, wid,
change, ...}` with `change` `state` (`state`, `bounds`: minimize, restore, maximize, snap, moved bounds), `closed`, or `focus` (`z`,
`state`). The shell forwards it to that app's frames only, without `id`.

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
| `os_window_info` | | any | `{label, app, dev, platform, groupWindows}` (`app` is the id or null; `platform` is the host OS, e.g. `windows`, `android`: the shell's layout; `groupWindows` is `bos.taskbar.groupWindows`, os.json `{"taskbar": {"groupWindows": false}}`, default true: the taskbar groups an app's windows under its button) |
| `os_main_inset` | | main | `{top, right, bottom, left}` (BOS-LAYOUT-1: logical px the maximized OS window reaches past its monitor's work area: an undecorated window keeps its resize frame, so maximized it overflows the screen; the shell pads itself by this so its taskbar stays on screen above the Windows taskbar; all 0 when not maximized) |
| `os_list_apps` | | main | `[{id, version, state, channel, name, icon, available, previous}]`: `name` from the installed manifest; `icon` its icon file as a `data:` URL (null when there is none, it is over 256 KiB or it is not a png, svg, jpg, webp, gif or ico); `available` the newer version the last feed read offered, or null (no network at this call); `previous` the version a rollback goes back to, or null |
| `os_app_status` | `id` | main, `app-<id>` | `{id, state, pid, url, paused, restarts, lastError}` |
| `os_app_health` | `id` | main, `app-<id>` | health, as text |
| `os_start_app` | `id` | main | status |
| `os_stop_app` | `id` | main | status |
| `os_open_app_window` | `id` | main | nothing. Opens the app **inside the BattalionOS window** (BOS-17): checks the app is installed and has a page, brings `main` to the front and emits `os://app-open` (the app id); the shell then fills its app area with the app's page in a frame. It opens no window of its own (the per-app window code stays for the pop out). The name is frozen |
| `os_close_app_window` | `id` | main, `app-<id>` | nothing; the app process keeps running |
| `os_catalog` | | main | `{apps:[{id, source, compatible, name, publisher, summary, icon, version}], rejected, sourceErrors, fromCache}`: `icon` only when the catalog gives a `data:image/` or `https:` URL; `version` is what the app's stable feed offers (null when the feed cannot be read) |
| `os_local_repos` | | main | LOCAL-REPO-1: `{repos:[{name, path, registered}], label}`: the private local repos (folders under `<home>/local-repos/`, plus os.json `armory.localRepos`). `os_catalog` entries from them carry `fromLocalRepo: true` and `label` "Local, built on this machine" |
| `os_local_repo_add` | `name` | main | `{name, path, registered}`; makes an empty repo folder |
| `os_local_repo_rename` | `name`, `newName` | main | `{name, path, registered}`; a repo set in os.json is refused |
| `os_local_repo_remove` | `name` | main | `{ok}`; deletes the repo folder (apps installed from it stay); a repo set in os.json is refused |
| `os_local_repo_link` | `name`, `path`? | main | ARMORY-LOCAL-2: `{name, path, registered, linked}` or `{cancelled}`; lists a folder kept elsewhere (a local checkout with catalog.json, e.g. shaped like battalionOS-apps; channel and package urls may be relative paths) as a local repo. No `path`: the folder picker. Its entries keep their own publisher key; removing the repo removes only the link, never the folder |
| `os_install_from_folder` | `path`?, `file`?, `allowUnsigned`?, `replaceCatalog`? | main | ARMORY-LOCAL-2 "Install from folder…": installs the app in a folder (app.json, or a .bosapp in it or its dist/) or a .bosapp file. No `path`: the picker (a file with `file`). Same package, manifest and pages-only checks as a catalog install; a present signature must verify. `{ok, id, version, from, signing}`, `{cancelled}`, or `{confirm: "unsigned"\|"replacesCatalog", message, path}`: call again with that `path` and the flag once the person agreed. `os_list_apps` entries installed this way carry `local: {path, signing}` |
| `os_install_app` | `id`, `channel?` (default `stable`) | main | `{ok}` |
| `os_open_login` | | main | nothing |
| `os_login_done` | | login | nothing; closes the login window, emits `os://login-changed` |

| `os_update_app` | `id`, `now?` | main | `{id, from, to}`, or `{id, notReady: true, version, reason, etaMs}` when the running app did not get ready within `armory.prepareWaitMs` (UPDATE-LIVE-1: the new version is staged first, the app is asked to get ready and the update applies by itself once it is; `now: true` = "Update now", the app checkpoints at once). Refused: up to date, bad signature, other key. Stops and starts only this app; emits `os://app-updated` when it is done |
| `os_set_app_channel` | `id`, `channel` | main | nothing. Moves an installed app to another channel its catalog entry lists (refused: not installed, or a channel the entry does not list). Installs nothing: only the app's channel is saved, then the channel's feed is read (a channel with no feed is no offer, not an error). An update from there still refuses a version not newer than the installed one |
| `os_update_later` | `id` | main | `{id, pending}`: "Later". The app is told to carry on; the version applies by itself at the app's next `os_start_app` or the next moment it is idle (checked every `armory.pendingCheckMs`) |
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

`os_list_apps` rows also carry `badge`: the running app's badge text, or null; `channels`: the channel names the app's catalog entry lists (cached catalog, empty when unknown); and `channelVersion`: the version the app's channel's cached feed names, newer or not, or null when that channel has no build yet.

Added by BOS-17 (an app opens inside the window). Both are for `main` only (the shell). An app's page is drawn by the shell in a frame, so it
has no window label of its own: the shell takes the app id from WHICH frame sent a message, and acts for that app with these.

| Command | Args | Who | Returns |
|---|---|---|---|
| `os_app_page` | `id` | main | `{id, version, origin, url}`: starts (or reuses) the page server of app `id` on the app's one kept page port and gives the frame's origin and entry url. An update replaces the server on the same port |
| `os_frame_badge` | `id`, `count?`, `text?` | main | nothing. `os_badge` for the app of a frame (same rules for `count` and `text`) |

### The page in a frame (BOS-17)

- The page is drawn in a frame in the shell's app area, from its own origin `http://127.0.0.1:<port>` (the app's page server). The frame is
  hidden, never reloaded, on minimize or switch; Close removes it. An update loads the new version in the frame.
- **Android (AND-3b):** no page server and no port. `os_app_page` answers origin `http://<id>.bos.localhost` and adds `files`
  (`{root, pageDir, csp}`: the version folder, the entry page's folder, the same CSP); the app's slot serves the page from them by
  request interception (docs/apps/android.md).
- **Port and origin.** `battalion_host::uiport`: each app with a `ui` has one page port, chosen once and kept in `<home>/ui-ports/<id>`, the
  same between runs, updates and OS restarts. Before the app starts the OS gives the origin as `BATTALION_UI_ORIGIN` and `uiOrigin` in the
  context (docs/app-abi.md); the app lets exactly that origin reach its socket and refuses others (403).
- **CSP of the page server:** `default-src 'self'`; scripts, styles, images, fonts and fetch/socket may also come from the app's own url (read
  at each request, so a restarted app on a new port is followed). Nothing else.
- **Messages.** The page calls the OS with `window.parent.postMessage({bos: 1, id, cmd, args}, "*")`; the shell answers the frame with
  `{bos: 1, id, ok, result}` or `{bos: 1, id, ok: false, error}`. The app id is the frame's (the frame the shell made, found by
  `event.source`, and its origin must equal the frame's origin); an id in the message is never used. Allowed: `os_window_info`,
  `os_app_status`, `os_app_health`, `os_close_app_window`, `os_badge`, `os_show_desktop`, with today's results and arguments (`id`
  may be omitted or equal to the frame's own; any other is refused), and from BOS-25 the five native calls below. Everything else is refused. A frame the shell did not make gets
  nothing. The frame has no `main` command rights (the capability for `main` has no remote url).
- `os_close_app_window` from a frame = Close (the view ends; the app keeps running). `os_show_desktop` = Minimize (the view keeps its state).

### The window's title bar (BOS-27)

The OS window `main` has no Windows title bar (decorations off). The shell draws its own: black, thin border, the BattalionOS mark, the
name of the app on screen (if any), and minimize / maximize / close, always on top of the desktop, the Armory, first run and any open
app (an app's frame starts under it). Apps are not involved: no message, no manifest field. The bar is a Tauri drag region (drag, double
click to maximize); thin strips on every edge and corner (above the frames) resize the window; Windows snap works. The buttons do what
the Windows ones did: Close hides the window (the OS and the apps go on, see Lifetime). Only `main` has the window rights this needs
(`core:window:allow-minimize`, `-toggle-maximize`, `-start-dragging`, `-start-resize-dragging`, `-is-maximized`, and the existing
`-close`); an app's frame and app windows have none.

### Native calls of a page in a frame (BOS-25)

An app's page in a frame has no window of its own, so it asks the OS for what a window of its own would do. Same message form, same
identity rule (the frame that asked; an `id` in the message is never used, any other than the frame's is refused). Each call needs the
one permission of its kind in the app's installed `app.json` (`permissions`, read at every call); without it the call is refused with
the permission's name. Nothing else is added: files and the clipboard are the frame's own (`<input type=file>`, and `allow="clipboard-read; clipboard-write"` on the frame).

| Page message `cmd` | Args | Permission | Result |
|---|---|---|---|
| `os_pick_folder` | `title?` | `files.user` | the chosen folder path, or null when cancelled |
| `os_pick_file` | `title?` | `files.user` | the chosen file path, or null when cancelled |
| `os_open_external` | `url` | `network` | nothing; opens the system browser. Only `http`, `https` and `mailto` (no spaces or control characters, up to 2048 bytes); any other is refused |
| `os_login_window_open` | `url` | `login.window` | nothing; the OS opens a window (no capability, its own data folder `webview/<id>-login`) showing `url` (`http`/`https` only). A second call replaces it |
| `os_login_window_close` | | `login.window` | nothing; closes it (none open is not an error) |
| `os_notify` | `title`, `text?` (or `body?`), `tag?`, `route?` | `notifications` | nothing; a notice for the app. Phone: on the app's own channel (AND-4). Desktop (BOS-NOTIFY-1): a Windows toast naming the app, shown without taking the focus; a repeat with the same `tag` replaces the shown one; at most 10 a minute per app. A click restores BattalionOS, brings the app forward and sends its frame `{bos: 1, event: "os_notify_click", tag, route}`. `title` up to 80 characters, `text` up to 400, `tag` up to 64 of `A-Za-z0-9_.:-`, `route` up to 512 |
| `os_scan_code` | | `scan.code` | the text of the QR code the OS scanned, or null when cancelled (AND-4, phone only); the page never gets the camera |

The shell serves them with these main-only commands (the app id is the frame's):

| Command | Args | Who | Returns |
|---|---|---|---|
| `os_frame_pick_folder` | `id`, `title?` | main | path or null |
| `os_frame_pick_file` | `id`, `title?` | main | path or null |
| `os_frame_open_external` | `id`, `url` | main | nothing |
| `os_frame_login_open` | `id`, `url` | main | nothing |
| `os_frame_login_close` | `id` | main | nothing |
| `os_frame_notify` | `id`, `args` (the page's `os_notify` args) | main | nothing; the desktop toast (BOS-NOTIFY-1). A click emits `os://app-open` and `os://frame-notify-click` |
| `os_frame_phone_call` | `id`, `cmd`, `args?` | main | `{call, args}` checked for the app's slot (AND-4) |

Events to the shell: `os://frame-login-url` `{id, url}` (the sign-in window reached a web address) and `os://frame-login-closed` `{id}`.
The shell sends them on to that app's frame only, as `{bos: 1, event: "os_login_url", url}` and `{bos: 1, event: "os_login_closed"}`.
`os://frame-notify-click` `{id, tag, route}` (BOS-NOTIFY-1, a desktop notice was clicked) goes to that app's frame only as
`{bos: 1, event: "os_notify_click", tag, route}`.

#### On a phone (AND-4)

The same page messages, the same `answer()` in the shell: the shell asks `os_frame_phone_call` (the host reads the app's installed
`app.json`, refuses an undeclared call with the permission's name, and cleans the arguments), then hands `{call, args}` to the app's
own slot, which runs it with Android and answers through the hub:

| `cmd` | On Android | Result |
|---|---|---|
| `os_pick_file` / `os_pick_folder` | the system document picker (Storage Access Framework); the slot keeps read access | a `content://` address (a phone has no file paths), or null |
| `os_open_external` | an intent (`ACTION_VIEW`; `mailto` = `ACTION_SENDTO`) | nothing; "No app on this phone opens that link" when none does |
| `os_notify` | a notification channel per app (`bos-app-<id>`, named after the app); `POST_NOTIFICATIONS` is asked at the first notice | nothing; refused with a plain sentence when the person says no |
| `os_scan_code` | the Google code scanner (QR only); BattalionOS asks for no camera permission and the page gets only the text | the text, or null |

`process.spawn` is never granted on a phone, even when `app.json` declares it: any call that would start a program is refused
("starting a program is never allowed on a phone"). The sign-in window calls are not served on a phone yet.
### File calls of a page in a frame (FS-API-1)

BattalionOS has ONE root folder: the setting `fs.root` (os.json `{"fs": {"root": "D:\\Data"}}`), by default `BattalionOS` in the
user's home folder (`%USERPROFILE%\BattalionOS`, `$HOME/BattalionOS`), created when missing and shown in Settings (Start menu).
An app with `files.read` in its installed `app.json` may reach anything under it, nothing outside it. Same message form and identity
rule as BOS-25. `files.user` (the pickers) is unchanged and does not grant these calls. The write side is FS-API-2.

Every `path` is RELATIVE to the root, `/`-separated; `""` (or no `path`) is the root itself. It is resolved once, in
`battalion-host::fs`. Refused (`bad_path`): absolute paths, a drive or a stream (`:`), `\`, `.` or `..` segments in any position,
empty segments, control characters, `< > " | ? *`, a name ending in a dot or a space, device names (`CON`, `PRN`, `AUX`, `NUL`,
`COM0`-`COM9`, `LPT0`-`LPT9`, `CONIN$`, `CONOUT$`, with any extension or case), and a path longer than `fs.maxPathChars`. Then the
file is opened and the open handle's real path (links and junctions followed) must still be under the root, checked at the open so a
link swapped in after a check cannot win (`leaves_root`). A link or junction inside the root to a place inside it is followed; one
pointing out is listed (`kind: "link"`) but never followed. The OS's own home is out of reach even when it lies under the root.

| Page message `cmd` | Args | Permission | Result |
|---|---|---|---|
| `os_fs_list` | `path?`, `cursor?`, `limit?` | `files.read` | `{entries: [{name, kind: "file"\|"dir"\|"link", size, modified}], cursor}` sorted by name; `cursor` is null on the last page, else passed back for the next; at most `fs.listMax` a page |
| `os_fs_stat` | `path?` | `files.read` | `{name, kind, target: "file"\|"dir", size, modified, created, readonly, display}`; `display` is a path for a person to read, never to be passed back |
| `os_fs_read` | `path`, `offset?`, `length?`, `as?: "text"\|"base64"` | `files.read` | `{data, size, offset, length, eof}`; at most `fs.readMaxBytes` an answer (read on from `offset + length`). `text` (the default) needs UTF-8 (`not_text` otherwise) and ends a chunk on a character boundary |
| `os_fs_watch` | `path?` | `files.read` | `{watch}`; the path itself and, for a folder, its direct entries are watched (every `fs.watchPollMs`). Changes come to that app's frame only as `{bos: 1, event: "os_fs_changed", watch, path, change: "created"\|"changed"\|"removed"}`. At most `fs.watchMax` an app |
| `os_fs_unwatch` | `watch` | `files.read` | nothing; another app's watch id is `no_such_watch` |

An error is a string ending in its code in brackets, e.g. `path leaves the OS root [leaves_root]`: `bad_path`, `leaves_root`,
`not_found`, `not_a_folder`, `not_a_file`, `not_text`, `too_many_watches`, `no_such_watch`, `bad_args`, `no_root`, `io`. No error
names an OS path.

Settings (os.json `fs`, each with a default): `root` (`<home>/BattalionOS`), `readMaxBytes` (4194304), `listMax` (1000),
`watchMax` (32), `watchPollMs` (1000), `maxPathChars` (1024).

The shell serves them with these main-only commands:

| Command | Args | Who | Returns |
|---|---|---|---|
| `os_frame_fs` | `id`, `cmd` (one of the `os_fs_*` above), `args?` | main | the call's result; the host checks `files.read` in the installed manifest of app `id`. Desktop only for now |
| `os_fs_info` | | main | `{root}`: the OS root folder's real path (created when missing), for Settings |

Event to the shell: `os://frame-fs-changed` `{id, watch, path, change}`; the shell sends it on to that app's frame only (above).

### File calls, write side (FS-API-2)

Same root, path rules, message form and `os_frame_fs` route as FS-API-1; these need `files.write` (which `files.read` does not
grant, nor the other way round). The parent of every target is opened and checked like a read (TOCTOU), and the last name is acted
on WITHOUT following it: a link or junction there is renamed or deleted as the link, its target is untouched. Both ends of a rename
or copy must be under the root. The root itself (`""`) and the OS's own home are never written over, renamed, trashed or deleted.

| Page message `cmd` | Args | Permission | Result |
|---|---|---|---|
| `os_fs_write` | `path`, `data`, `as?: "text"\|"base64"`, `overwrite?` (default true) | `files.write` | `{size}`; atomic: written to a hidden temp beside the file, flushed, then renamed over it, so a reader sees the old file or the new one, never a part. At most `fs.writeMaxBytes` (`too_large`). `overwrite: false` refuses an existing name (`exists`) |
| `os_fs_mkdir` | `path` | `files.write` | nothing; one folder, its parent must exist; `exists` if taken |
| `os_fs_rename` | `from`, `to` | `files.write` | nothing; moves or renames; never over an existing name (`exists`), never a folder into itself |
| `os_fs_copy` | `from`, `to` | `files.write` | `{entries}`; a file or a whole folder to a new `to`. Every source is checked before anything is written (a link out anywhere inside fails the copy, `leaves_root`); built in a temp beside `to` and renamed into place, so a failed copy leaves nothing. At most `fs.copyMaxEntries` |
| `os_fs_trash` | `path` | `files.write` | nothing; to the OS recycle bin (Windows). A link is refused (delete it). Elsewhere `io` until a bin is wired |
| `os_fs_delete` | `path`, `recursive?` | `files.write` | nothing; a file, an empty folder, a folder with `recursive: true` (`not_empty` otherwise), or a link/junction, which is removed as the link. A recursive delete never goes through a link inside |

More error codes: `is_root`, `exists`, `too_large`, `not_empty`. More settings: `writeMaxBytes` (16777216), `copyMaxEntries` (10000).

When the shell closes an app's frame it calls `os_frame_fs_close` and the app's watches end; stopping the app drops them too.

| Command | Args | Who | Returns |
|---|---|---|---|
| `os_frame_fs_close` | `id` | main | nothing; drops every file watch of app `id` |

### The app's own store (APP-API-1)

A small key/value store for each app (settings, the last folder, recent items), kept in the app's own state folder
`<home>/data/<app>/store.json` (`{"schema": 1, "values": {...}}`, written atomically). No permission: the store is always the
calling frame's app's own, the app is never an argument, so an app can never read or change another app's store. It survives
updates and is kept on remove like the rest of the state folder. Same message form as the file calls; the client package is
[`@battalionos/app-api`](apps/app-api.md). Schema: `crates/battalion-app-api/schemas/store.schema.json`.

| Page message `cmd` | Args | Permission | Result |
|---|---|---|---|
| `os_store_get` | `key` | none | the value, or `null` when the key is not set |
| `os_store_set` | `key`, `value` (any JSON) | none | nothing; `value: null` deletes the key. Refused with nothing written when the file would pass `store.maxBytes` (`too_large`) |
| `os_store_delete` | `key` | none | `true` when the key was there, else `false` |
| `os_store_list` | `prefix?` | none | the keys, sorted, that start with `prefix` (all without one); at most `store.listMax` |

A key is 1 to `store.maxKeyChars` characters, no control characters (`bad_args`). Error codes (the `[code]` at the end of the
message): `bad_args`, `too_large`, `bad_app`, `io`. Settings (`store` in `os.json`): `maxBytes` (1048576), `maxKeyChars` (256),
`listMax` (10000).

| Command | Args | Who | Returns |
|---|---|---|---|
| `os_frame_store` | `id`, `cmd` (one of the `os_store_*` above), `args?` | main | the call's result, on app `id`'s own store; `id` must be an installed app |

### Open with (OPEN-WITH-1)

An app declares the files it opens in `app.json` `fileTypes` ([create.md](apps/create.md#every-field)). Any app's page may ask
the OS to open a file under the OS root in another app. The caller needs NO permission beyond knowing the path, but the path
must resolve (links followed) to a file under the OS root (`battalion-host::fs`, the one place paths are checked:
`bad_path`, `leaves_root`, `not_found`, `not_a_file`).

The OS picks the receiver: the app named by `appId` (it must be installed, have a page and declare a type that covers the
file, else `no_such_app`); else the remembered default for the file's extension, while it still covers it; else the person
chooses in the shell's "Open with" chooser, which lists only the installed apps with a page whose `fileTypes` cover the file
(by extension, or by the MIME type the OS looks up from the extension; `text/*` covers every text type). The chooser is shown
even when one app matches: the person, not the OS, decides who receives a file. "Always use this app" remembers the choice
per extension in `<home>/state/open-with.json` (`{"schema": 1, "defaults": {"md": "notes"}}`); Settings list the defaults with
Forget. No matching app at all is `no_app`.

The receiver's view is opened (`os://app-open`) and the file is queued for it. Its page takes the queued paths with
`os_open_file_take` (at load, so a file handed over before the page ran is not lost) and is told of each new one by an
`{bos: 1, event: "os_open_file", path}` message. **A one-file grant, not `files.read`:** when the receiver lacks `files.read`,
the OS grants it `os_fs_read` and `os_fs_stat` of exactly that path and nothing else (not its folder, not a list, not a
write), until its frame closes or the OS exits. Least privilege: an editor that is only ever handed files needs no reach over
the whole root, and the opener never hands over more than the one file the person chose. Writing the file back needs
`files.write` (a later save grant may lift that). An app with `files.read` gets no grant (it needs none).

| Page message `cmd` | Args | Permission | Result |
|---|---|---|---|
| `os_open_with` | `path`, `appId?` | none | `{opened, path}`: the app that received it, or `opened: null` when the person cancelled the chooser |
| `os_open_file_take` | | none | the paths handed to this app since it last asked, oldest first, each once |

Error codes: `bad_args`, `no_such_app`, `no_app`, `bad_path`, `leaves_root`, `not_found`, `not_a_file`, `no_root`, `io`.
Schema: `crates/battalion-app-api/schemas/open.schema.json`. Not on a phone yet ("not available on this device yet").

| Command | Args | Who | Returns |
|---|---|---|---|
| `os_frame_open_with` | `id` (the calling frame's app), `args: {path, appId?}` | main | `{opened, path}`, or `{choose: [{id, name, label}], path, ext}` when the person must choose |
| `os_open_with_pick` | `path`, `appId`, `always?` | main | `{opened, path}`: the person's choice in the chooser (the app must still cover the file); `always` remembers it |
| `os_open_with_defaults` | `clear?` (an extension) | main | `{ext: appId}`, the remembered defaults, after forgetting `clear` |
| `os_frame_open_file_take` | `id` | main | the paths handed to app `id` (`os_open_file_take`) |

## Events (emitted to all windows)

| Event | Payload |
|---|---|
| `os://app-state` | `{id, state}` after `os_install_app`, `os_update_app`, `os_rollback_app` and `os_remove_app` end (`state` is null for a removed app) |
| `os://armory-progress` | `{id, op, stage, bytes, total, percent, reason?, etaMs?}`: `op` is `install`, `update`, `rollback` or `remove`; `stage` is the step (`download`, `verify`, `unpack`, `place`, `prepare`, `init`, `stop`, `switch`, `start`, `done`, `mark`, `delete`, `forget`); `bytes`, `total` and `percent` (0-100) only while `stage` is `download`, else null; `reason` and `etaMs` only while `stage` is `prepare` (the update waits for the app: "<app> will update when <reason>") |
| `os://app-updated` | `{id, from, to}` after an update applied (by `os_update_app`, or a pending one): the shell closes the app's view and opens it again on the new version |
| `os://app-open` | the app id; the shell opens the view of that app inside its window |
| `os://open-file` | `{id, path}` (OPEN-WITH-1): a file was handed to app `id`; the shell tells that app's frame only (`os_open_file`) |
| `os://app-window-closed` | the app id |
| `os://login-changed` | `true` |
| `os://frame-login-url` | `{id, url}` (BOS-25) |
| `os://frame-login-closed` | `{id}` (BOS-25) |
| `os://badge` | `{id, badge}` when an app sets or clears its badge (`badge` is the text or null) |
| `os://win` | `{bos: 1, event: "os_win", id, wid, change, ...}` (WM-2): to the owning app's windows only, not to all windows (see Window calls) |

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
| `os_take_first_run_notice` | | main | the ids the first run just installed, once, else `[]`; the shell names them in one line ("Infantry AI is installed with BattalionOS. You can remove it in the Armory.") |

Event `os://first-run` `{status, ids?, id?, error?}`: `start` (ids), `installed` (id), `done` (ids), `offline` or `failed` (id, error), `already-done`, `nothing`.

Added by BOS-19 (start from outside). `BattalionOS --open <app-id>` starts the OS, or hands the request to the running one
(single instance), and the app opens inside the window as by its icon. The id is checked (the app id grammar); an app that is not
installed shows the Armory; every other argument is ignored. The OS writes one Start menu shortcut per installed app
(`Start Menu\Programs\BattalionOS Apps\<name>.lnk`, running `BattalionOS.exe --open <id>`, the app's `.ico` or the OS's icon): after an install,
update, rollback or remove, and at every start. The map of what it wrote is `shortcuts.json` in the OS home.

| Command | Args | Who | Returns |
|---|---|---|---|
| `os_take_open_request` | | main | the app id of a pending `--open`, once, else `null` |

Event `os://open-request`: a second start asked for an app; the shell calls `os_take_open_request`.

## BattalionOS updates itself (BOS-30)

The OS has its own signed feed (`latest.json`, address and key as data in `tauri.conf.json`, `plugins.updater`). It is not an app:
the Armory shows it as one row in Installed, with no Open and no Remove, and it is never an argument of an app command.

| Command | Args | Who | Returns |
|---|---|---|---|
| `os_update_state` | | main | `{current, available: {version, notes} \| null, installing, busy: [{id, name, busy}], running: [id]}`; no network |
| `os_update_check` | | main | the same, after reading the feed. No feed, 404 or offline: `available` stays as it was, no error |
| `os_update_install` | | main | downloads and verifies the signature first; only then stops every app, starts the installer (per user, no admin) and ends the OS. The installer starts the OS again; the apps that ran start again. A bad signature, a broken download or a failed stop is an error and nothing was stopped |
| `os_update_abort` | | main | Android only (BOS-AND-SELFUPDATE-1): Android did not install the update (cancelled, wrong signing certificate, no permission to install): the apps the update stopped start again and the downloaded APK is removed. On Android `os_update_install` answers `{apk, sha256, version}` after the sha256 check and the app stop; the shell hands it to Android's PackageInstaller (`bosSlots` op `osInstall`) |

Event `os://os-update` (`{current, available}`): a check finished (when the Armory opens, and once a day). Progress of the install is
`os://armory-progress` with `id` `battalionos`, `op` `update`. Test builds only (cargo feature `test-update`): `BATTALION_OS_UPDATE_FEED`
and `BATTALION_OS_UPDATE_PUBKEY` replace the feed address and the key.

## Developer builds (BOS-31)

A build with the cargo feature `dev` (never a release build) adds: `os_window_info` answers `dev: true`; the event `os://dev-app` (`{id, status: started|restarted|failed, ms?, error?}`); and
the command line `--dev-app <folder> [--dev-url <loopback url>]`. No command is added. See [apps/create.md](apps/create.md#develop-with-live-reload).

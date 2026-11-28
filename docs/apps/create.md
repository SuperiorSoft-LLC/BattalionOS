# Create an app: app.json

New here? Start with [Make your own app](make-your-own-app.md): a template, a typed client ([`@battalionos/app-api`](app-api.md)) and a packer, so you need nothing but this repo and Node.

Every app has one manifest, `app.json`, at the root of its package (or of its folder while you develop it). This page
lists every field, from `crates/battalion-app-api/src/manifest.rs` (`Manifest`), then shows how to run an app from its
folder with live reload. What an app is: [overview.md](overview.md).

## A small app.json

A page-only app (runs on Windows and Android alike; this is `fixtures/page-app/app.json`):

```json
{
  "manifest": 1,
  "id": "page-app",
  "name": "Page App",
  "version": "1.0.0",
  "publisher": "BattalionOS",
  "license": "Apache-2.0",
  "contract": { "min": 1, "max": 1 },
  "minOs": "0.1.0",
  "ui": { "entry": "ui/index.html" },
  "state": { "schema": 1 }
}
```

An app with a process part adds, per platform:

```json
"process": {
  "windows-x86_64": {
    "library": "lib/my_app.dll",
    "args": [], "env": {}, "envScrub": ["MYAPP_"],
    "readyTimeoutMs": 60000, "stopTimeoutMs": 15000
  }
}
```

## Every field

Unknown fields are ignored, so a newer manifest still loads on an older OS. Every path in the manifest is relative to
the package root and must stay inside it: no leading `/` or `\`, no drive (`:`), no `..`, no empty segment; anything
else is refused ("<field> path '...' leaves the package").

| Field | Type | Required | Meaning and check |
|---|---|---|---|
| `manifest` | number | yes | The manifest format. Must be `1`. |
| `id` | string | yes | The app's id: `a-z`, `0-9`, `-`, 2 to 63 characters, starting with a letter or digit. It names the app's folders and its catalog entry; the package's `id` must match the catalog's. |
| `name` | string | yes | The name shown to people. Not empty. |
| `version` | string | yes | `major.minor.patch`. Must match the feed's `version` at install. |
| `publisher` | string | no | Who makes the app. |
| `license` | string | no | The app's licence (an SPDX id). |
| `icon` | path | no | An image in the package, shown on the desktop. |
| `contract` | `{min, max}` | yes | The contract versions the app speaks. `min` ≥ 1 and `min` ≤ `max`; the OS's contract (today `1`) must be in the range, or the app is refused ("app needs contract min..max, this OS speaks N"). |
| `minOs` | string | no | The lowest BattalionOS version, `major.minor.patch`. |
| `process` | map | no | One entry per platform key: `windows-x86_64`, `macos-aarch64`, `linux-x86_64`, ... (see below). No entry for the platform = no process part there. |
| `ui` | object | no | The app's page (see below). |
| `init` | object | no | A program in the package run once at install, to its end, in the app's state folder (see below). Refused on Android. |
| `state` | object | no | Where the app's data lives (see below). |
| `cli` | list | no | Command-line programs the app ships: `[{ "name", "exec" }]`. `name` follows the `id` rule; `exec` is a path in the package. The OS never starts them. |
| `permissions` | list | no | What the app's page may ask the OS for. Known values: `process.spawn` (never granted on a phone), `network`, `files.user`, `window`, `login.window`, `notifications`, `scan.code`, `files.read` (FS-API-1: list, stat, read and watch anything under the OS root folder, the `fs.root` setting; see [commands.md](../commands.md#file-calls-of-a-page-in-a-frame-fs-api-1); `files.user` does not grant it, and it does not grant the `files.user` pickers), `files.write` (FS-API-2: write, mkdir, rename, copy, trash and delete under the OS root; see [commands.md](../commands.md#file-calls-write-side-fs-api-2); it does not grant `files.read`, so a file manager asks for both). An unknown value is refused, so a typo never grants nothing silently. The page's native calls are gated by this list ([commands.md](../commands.md#native-calls-of-a-page-in-a-frame-bos-25)). |

| `fileTypes` | list | no | OPEN-WITH-1: the files the app opens, offered in the OS's "Open with" chooser: `[{ "label", "extensions"?, "mime"? }]`. `label` is what a person reads ("Markdown document", 1 to 80 characters); `extensions` are lower case without the dot (`"md"`; `a-z`, `0-9`, `-`, `_`, `+`, up to 16); `mime` are `type/subtype` or `type/*` (`"text/plain"`, `"image/*"`), matched through the MIME type the OS looks up from the file's extension. Each entry names at least one extension or MIME type. A malformed entry refuses the manifest; an unknown field inside an entry is ignored, and an OS older than OPEN-WITH-1 ignores `fileTypes` entirely. Needs a `ui`: the file reaches the app's page. See [commands.md](../commands.md#open-with-open-with-1). |

At least one of a process part for the running platform or a `ui` is required.

### process.&lt;platform&gt;

The platform key is `<os>-<arch>` as the OS reports it (`windows-x86_64`, `linux-x86_64`, `macos-aarch64`). Android is
always `android-aarch64` ([android.md](android.md)).

| Field | Default | Meaning |
|---|---|---|
| `library` | required | The app's library in the package. Its extension must fit the platform: `.dll` (windows), `.dylib` (macos), `.so` (linux). The OS loads it on the app's own worker and calls its `run` ([app-abi.md](../app-abi.md)). |
| `exec` | | **Refused.** An app ships a library, not a program: "process.&lt;platform&gt;.exec is not allowed". |
| `args` | `[]` | Arguments handed to the app in its run context. |
| `env` | `{}` | Environment set for the app. |
| `envScrub` | `[]` | Prefixes (`A-Z`, `0-9`, `_`; e.g. `MYAPP_`) of the OS's own environment the app must not inherit. `BATTALION_*` is always removed, then the contract values are set. |
| `readyTimeoutMs` | `60000` | How long the app has to write its ready file. |
| `stopTimeoutMs` | `10000` | How long after a stop request before the OS ends the worker's job. |

### Prepare for an update

Optional (UPDATE-LIVE-1). An app that would lose work when it is stopped names a path in its ready file:
`"prepare": "/prepare-update"`. Before an update, the OS calls `POST <url><prepare>` with the per-start secret
(`tokenFile`) as `Authorization: Bearer`, and a JSON body:

| Body | The app does |
|---|---|
| `{"to": "<version>"}` | Gets ready: stops starting new work, lets work in progress finish. Answers `{"ready": true}` when it can be stopped without losing anything, else `{"ready": false, "etaMs": <ms>, "reason": "<phrase>"}`. The OS asks again until it is ready, so every call is idempotent. |
| `{"to": "<version>", "now": true}` | "Update now": saves what it needs to resume the work it cuts, then answers `{"ready": true}`. |
| `{"cancel": true}` | The update is not happening now: back to normal work. |

`reason` completes the sentence "<App name> will update when ...", e.g. `"its agents finish their current step"`. After
`ready`, the OS stops the app the normal way, switches the version and starts it. An app without `prepare` is waited for
until its health answer says `busy` 0.

### ui

| Field | Meaning |
|---|---|
| `entry` | Required. The page's HTML file in the package, e.g. `ui/index.html`. |
| `window` | Optional `{ "width", "height" }`: the size of the app's window when popped out. |
| `connect` | Optional list of sources added to the page's Content-Security-Policy `connect-src` (besides its own origin), e.g. `"ws://127.0.0.1:*"`. |

An app with a `ui` and a process part gets the page's origin (`http://127.0.0.1:<port>`, the same port between runs)
in its run context as `uiOrigin` and as `BATTALION_UI_ORIGIN`; it should let exactly that origin reach its socket.

### init

| Field | Meaning |
|---|---|
| `exec` | Required. A program in the package. |
| `args` | Optional arguments. |

The install runs it to its end, in the app's state folder, with a log at `logs/<id>-init.log`. A failing or hanging
init (past `armory.initTimeoutMs`, default 120000) fails the install, and nothing is left installed. An OS that runs
pages only (Android) refuses an app with `init`.

### state

| Field | Default | Meaning |
|---|---|---|
| `dir` | `<os home>/data/<id>` | Per OS (`windows`, `linux`, `macos`): a folder the app already owns, e.g. `"%LOCALAPPDATA%\\MyCo\\MyApp"`. |
| `schema` | `1` | The version of the app's data layout. When an update raises it and the new version fails to start, the OS does not roll back onto changed data: the app is left failed for a person to decide ([armory.md](armory.md#update-and-rollback)). |

Removing an app keeps its state folder. A folder the app named itself is never deleted by the OS.

## Develop with live reload

Work on an app inside BattalionOS without packaging it, signing it, installing it or rebuilding the OS. This is
developer mode, only in a **developer build** of the OS (cargo feature `dev`, a compile-time choice): a release build
has no folder apps, no dev servers and no reload (`crates/battalion-host/tests/dev.rs`,
`a_release_build_refuses_a_folder_app`). Developer mode runs on Windows; it is not part of the Android build.

| You change | What happens |
|---|---|
| The app's page (its own dev server, any framework) | The OS window shows the page from your dev server, so its hot reload works inside the OS window. |
| The app's library (rebuilt) | The OS sees the new library, stops that app the normal way (its `stop`, the timeout, then its job) and starts the new one. Other apps keep running. |
| The OS's own screens (`crates/battalion-desktop/ui`) | Served by the OS's Vite dev server: hot reload. |
| The OS's Rust code (with `-OsWatch`) | Only the OS is rebuilt and restarted. |

### The app folder

A folder with what a package holds: `app.json`, the built library at the path `app.json` names, and the page folder
(`ui.entry`). No signature, no catalog entry, no `.bosapp`. The OS copies the library into the app's run folder before
it loads it, so your compiler can overwrite the original while the app runs (Windows locks a loaded library).

### Run it

From the BattalionOS checkout (PowerShell):

```powershell
scripts/bos-dev.ps1 -App C:\work\my-app -AppUrl http://127.0.0.1:5173   # -AppUrl: the app's own dev server, optional
scripts/bos-dev.ps1 -App C:\work\my-app -OsWatch                        # also rebuild + restart the OS when its Rust changes
scripts/bos-dev.ps1 -App C:\work\my-app -AppEnv PORT=0 -Open my-app     # an env pair for the last app; open it at start
scripts/bos-dev.ps1 -Stop                                               # stop the dev OS and its dev server, by pid
scripts/bos-dev.ps1 -Stop -DevRoot D:\my-dev-data                       # the same, when the start used -DevRoot
```

`-App`, `-AppUrl` and `-AppEnv` can be repeated for more apps.

- **Where it builds.** Into its own folder, `%LOCALAPPDATA%\bos-dev-target\os` (`-OsTargetDir` changes it), never the
  checkout's `target`. The first build takes several minutes; later starts rebuild only what changed.
- **A second start** while a dev OS runs from the same data folder starts nothing and prints the pid and the `-Stop`
  command. `-Stop` ends the watcher, the OS and the screens' dev server by the pids it recorded (never by image name).
- **Logs.** `<DevRoot>\logs`: `os.log`, `os-build.log` (its end holds build errors), `ui.log`. A failure prints one
  line and names the log.
- **Disk.** Below `-MinFreeGb` free GB (default 8) on the target drive the script stops with one line.

Without `-AppUrl` the OS serves the page from the folder (files are read at every request; reload to see a change).

Without the script: build `battalion-desktop` with `--features dev` and start it with
`--dev-app <folder> [--dev-url <url>] [--dev-env KEY=VALUE]...` (the url and env belong to the `--dev-app` before them).

### What dev mode keeps apart

- **Data.** The OS and each dev app use `-DevRoot` (default `%TEMP%\bos-dev`): `BATTALION_HOME` there, and `HOME`,
  `USERPROFILE`, `APPDATA` and `LOCALAPPDATA` there too. A dev app never uses the folder its `state.dir` names: that is
  the installed app's real data. `BATTALION_*` and the variables the script lists are removed from everything it starts.
- **Identity.** Its own single-instance id (`org.battalionos.desktop.dev`) and WebView data: it runs beside an
  installed BattalionOS without touching it. The window title carries a `dev` mark.
- **Ports.** Loopback only (`127.0.0.1`, `localhost`, `[::1]`); a dev url that is not loopback is refused.
- **Installed apps.** An app installed the normal way is never replaced by a folder app.

The page with a dev server gets the checks an installed app has: the frame's origin is the dev server's origin, a
message from another origin gets no answer, and `permissions` gate the native calls. The app is started with
`BATTALION_UI_ORIGIN` set to that origin.

### Reload and errors

The OS looks at the library file every 250 ms. A change that has stopped changing (a linker writes in steps) restarts
the app, and the title bar says `<app> restarted in N ms`. A library that does not load shows the error in the title
bar and in `logs/<id>.log`; the next good build starts the app again. An app you stopped yourself stays stopped.

Next: [package and sign the app](package-and-sign.md).

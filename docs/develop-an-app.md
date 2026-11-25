# Develop an app

How to work on an app inside BattalionOS without packaging it, signing it, installing it or rebuilding the OS.
This is developer mode (BOS-31). It exists only in a **developer build** of the OS (cargo feature `dev`, a compile-time choice):
a release build has no folder apps, no dev servers and no reload, and a test (`crates/battalion-host/tests/dev.rs`,
`a_release_build_refuses_a_folder_app`) shows it refuses one.

## What you get

| You change | What happens |
|---|---|
| The app's page (its own dev server, any framework) | The OS window shows the page from your dev server, so the page's own hot reload works inside the OS window. The OS is not touched. |
| The app's code, rebuilt into its library | The OS sees the new library, stops that app the normal way (its `stop`, the timeout, then its job) and starts the new one. Same OS process, other apps keep running. |
| The OS's own screens (`crates/battalion-desktop/ui`) | The window is served by the OS's Vite dev server: hot reload. The app is not rebuilt. |
| The OS's Rust code | Only the OS is rebuilt and restarted. The app is not rebuilt. |

An app is anything that ships a library (`process.<target>.library` in `app.json`, the [ABI](app-abi.md)) and, optionally, a page.
The OS names no app and no language.

## The app folder

A folder with what a package holds: `app.json`, the built library at the path `app.json` names, and the page folder (`ui.entry`).
Nothing else: no signature, no catalog entry, no `.bosapp`. The OS **copies the library** before it loads it (into the app's run
folder), so your compiler can overwrite the original while the app runs (Windows locks a loaded library).

## Run it

```powershell
# run from the BattalionOS checkout (builds the OS with the dev feature and starts it)
scripts/bos-dev.ps1 -App C:\path	opp-folder -AppUrl http://127.0.0.1:5173     # -AppUrl: the app's own dev server, optional
scripts/bos-dev.ps1 -App C:\path	opp-folder -OsWatch                           # also rebuild + restart the OS when its Rust changes
scripts/bos-dev.ps1 -Stop                                                         # stop the dev OS and its dev server, by pid
scripts/bos-dev.ps1 -Stop -DevRoot D:\my-dev-data                                 # the same, when the start used -DevRoot
```

- **Where it builds.** The dev OS is built into its own folder, `%LOCALAPPDATA%os-dev-target\os` (change it with `-OsTargetDir`), never the checkout's
  normal `target`. The script says the folder at the end. The first build takes several minutes; later starts rebuild only what changed.
- **A second start.** While a dev OS runs from the same data folder, running the script again starts nothing and prints the pid and the `-Stop` command.
  `-Stop` ends the watcher, the OS and the screens' dev server, by the pids it recorded (never by image name), and tells you what it ended.
- **Logs.** `<DevRoot>\logs`: `os.log` (the OS), `os-build.log` (the build; its end holds the errors), `ui.log` (the screens' dev server).
  A failure prints one plain line and names the log.
- **Disk.** Below `-MinFreeGb` free GB (default 8) on the target folder's drive the script stops with one line instead of starting a build.
- **Times.** The summary prints how long the start took. With `-OsWatch` each rebuild prints `OS rebuilt and restarted in N s` (about 5 s for a small change).
  A change to the OS screens shows in under a second.

Without `-AppUrl` the OS serves the page from the folder (files are read at every request; reload the page to see a change).

The same thing without the script: build `battalion-desktop` with `--features dev` and start it with
`--dev-app <folder> [--dev-url <url>] [--dev-env KEY=VALUE]...` (repeat for more apps; the url and env belong to the `--dev-app` before them).
An env pair is for what the app would otherwise take from a fixed place, e.g. a free port for its own server (`-AppEnv KEY=VALUE` of the script).

### What dev mode keeps apart

- **Data.** The OS and each dev app use their own folder (`-DevRoot`, default `%TEMP%\bos-dev`): `BATTALION_HOME` there, and the OS runs with `HOME`,
  `USERPROFILE`, `APPDATA` and `LOCALAPPDATA` there too, so nothing of your tools or of an installed app is read or changed. A dev app never uses the data
  folder its manifest names (`state.dir`): that is the installed app's real data.
- **Identity.** Its own single-instance id (`org.battalionos.desktop.dev`) and its own WebView data: it runs beside an installed BattalionOS without
  closing, focusing or replacing it. The window title and the title bar carry a `dev` mark.
- **Ports.** Free loopback ports only. Every dev server and every listener is on `127.0.0.1` / `::1`; a dev url that is not loopback is refused.
- **Installed apps.** An app that is installed the normal way in the data folder is never replaced by a folder app.

### The page, with a dev server

`--dev-url` must be `http://` or `https://` on `127.0.0.1`, `localhost` or `[::1]`. The frame loads it; the checks are the ones an installed app has:
the frame's origin is the dev server's origin, a message from any other origin gets no answer, and the app's manifest `permissions` gate the native calls.
The app is started with `BATTALION_UI_ORIGIN` set to that origin, so it lets exactly the dev server's page reach its socket.

### Reload and errors

The OS looks at the library file every 250 ms. A change that has stopped changing (a linker writes in steps) restarts the app, and the title bar says
`<app> restarted in N ms`. A library that does not load shows the plain error in the title bar (and in `logs/<id>.log`); the OS keeps watching, and the
next good build starts the app again. An app you stopped yourself is left stopped.

## Infantry AI

```powershell
# run from the infantry-ai checkout (a second checkout, next to BattalionOS): builds the library in dev form, lays the folder, starts the page's dev server and the dev OS
scripts/bos-dev-app.ps1 -Os C:\path\to\BattalionOS [-Watch]
```

`-Watch` rebuilds only the library when `rust/` changes and copies it into the folder; the OS restarts the app by itself.

The two checkouts: `scripts/bos-dev.ps1` is run from the BattalionOS checkout; `scripts/bos-dev-app.ps1` is run from the infantry-ai checkout and is given the BattalionOS checkout with `-Os` (it calls `bos-dev.ps1` there).

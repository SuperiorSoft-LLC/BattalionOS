# Packaging: the BattalionOS installer and first run

Two steps, like a normal install.

1. **The OS.** `BattalionOS_<version>_x64_en-US.msi` (per user, no admin). Its pages are the WixUI_InstallDir set plus
   one more, **Install apps**, with *Infantry AI (from the Infantry AI repository)* checked by default.
2. **The app.** The first time BattalionOS opens, it installs the apps that were checked, with progress, and their
   icons appear on its desktop.

How the checkbox reaches the OS: the box is the property `INSTALL_INFANTRY` (default 1). `first-run.json` is a
component with the condition `INSTALL_INFANTRY = "1"`, so it is laid beside the executable only when chosen.
Not chosen means no file, and the OS has nothing to do. Silent: `msiexec /i BattalionOS.msi /qn INSTALL_INFANTRY=0`.

`first-run.json` (data, edit it, not the code): `{"sources": [catalog urls], "apps": [{"id", "channel"}]}`. `sources` is
written to `os.json` (`armory.sources`) only when the person has none.

First open (`battalion-host/src/firstrun.rs`, wired in `battalion-desktop`):

| Case | Result |
|---|---|
| No `first-run.json` | nothing happens, no marker |
| Chosen | every listed app is installed through the Armory (all or nothing each), then `first-run.state.json` is written in the OS home: the done marker |
| Offline or fetch error | nothing half installed, no marker, `os://first-run` says `offline`; the next open or `os_first_run_retry` runs it again |
| An app fails (bad package) | the other apps still install; no marker; retry runs the rest |
| Killed mid-install | the Armory journal is recovered first (finished or undone), then the run resumes; the marker is written last |
| Already done | never runs again, so an app the person removed is not put back |
| Uninstall the OS | removes the install folder only; the OS home (apps, their state, the marker) stays |

Build the MSI (Windows; needs the UI built once, see `crates/battalion-desktop/ui`):

```
cd crates/battalion-desktop
pnpm --dir ui install --frozen-lockfile && pnpm --dir ui build
pnpm dlx @tauri-apps/cli@2 build --config ../../packaging/tauri.msi.conf.json --bundles msi
pwsh packaging/test-msi.ps1          # installs it silently both ways and checks the plan file
```

The template is `windows/main.wxs` (from the Infantry AI per-user template, cut down, plus the page); the plan
component is `windows/first-run.wxs`.

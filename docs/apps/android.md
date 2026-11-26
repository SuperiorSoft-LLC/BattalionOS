# Apps on Android

BattalionOS also builds as one Android app (`crates/battalion-android`, Tauri 2 mobile). That Android app **is the
OS**: the same shell page, the same Armory and the same app states as on the desktop. What differs is only what a
phone has. Truth: `crates/battalion-android/src/lib.rs`, `crates/battalion-host/src/armory/package.rs`
(`pages_only_check`), `crates/battalion-host/src/supervisor.rs` (`platform_key`, the page runner).

## An app on Android is a page-only app

An Android OS runs app **pages** only. An app runs on the page runner: same states as anywhere (starting, running,
stopped), no worker, no library, and it opens as its page in the shell's app area.

To run on Android an app needs:

- a `ui` in its `app.json` ([create.md](create.md#ui)) and no `init`;
- a feed entry for **`android-aarch64`** ([package-and-sign.md](package-and-sign.md#the-feed)). Every Android device,
  the x86_64 emulator included, takes that one entry, because the package holds no native code. Do not add an
  `android-aarch64` key under `process`: its library would be refused as native code.

A package for Android holds `app.json`, the page files and the icon. Signing, the catalog and updates are the same as on
the desktop ([armory.md](armory.md)).

## What Android refuses

The Armory's `pagesOnly` setting is on for Android (off elsewhere). With it, after the package is unpacked and before
anything is installed, the OS refuses:

| Refused | Message |
|---|---|
| An app with `init` | "the app runs a program at install (init)" |
| Any file whose first bytes say it is native code, judged by content, never by name | "the package holds native code (<kind>: <path>)" |

Native content it recognises: ELF libraries or programs (`\x7fELF`), Android dex code (`dex\n`), JAR or APK archives
(`PK\x03\x04`; a zip inside the package counts), Windows DLL or EXE (`MZ`), Mach-O programs, and Java classes. Keep the
page files plain (HTML, JS, CSS, images, fonts, JSON, wasm).

An app with no `ui` cannot run on Android: with no process part for `android-aarch64` and no page, its manifest is
refused ("app has neither a process part for this platform nor a UI part").

## What the phone shell does differently

- The shell keeps the desktop, the Armory and the taskbar (the app switcher). Each open app's page runs in its **own
  Android process slot** (AND-3), not in the shell's window: see "App slots" below.
- One tap opens an icon; the back gesture closes the Armory, then takes an open app back to the desktop.
- The layout keeps clear of the system bars, the cutout and the keyboard.
- No tray, no single-instance, no Start-menu shortcuts and no self update: the
  OS updates through its store.

## App slots (one process per app)

Truth: `crates/battalion-android/gen/android/app/src/main/java/org/battalionos/os/devtest/{SlotHub,AppSlot}.kt`,
`crates/battalion-desktop/ui/src/slots.ts`, `AppFrames.tsx`.

- The manifest declares a fixed pool of 4 slot activities, `AppSlot0..3`, each with its own process (`:app0`..`:app3`).
  Android creates and ends those processes; the OS's hub (`SlotHub`, in the OS's own process) gives a slot to an app
  while it is open and ends the slot's process before the slot serves another app.
- Each slot sets the WebView data folder suffix to the app id (`app_webview_app-<id>` under the OS's private folder), so
  an app's cookies, localStorage and cache are its own: one app cannot read another's, or the shell's.
- A slot shows a small OS-owned host page that holds the app's page in one frame at the app's own origin. The page
  contract does not change: the app still calls `window.parent.postMessage({bos: 1, ...})`, and the shell answers with
  the same `frames.ts` rules as on the desktop. The OS knows the app from the slot it gave, never from a message.
- The page is served by the slot itself, by request interception, at `http://<app-id>.bos.localhost/` (one host name
  per app, so each app has its own origin). No page server and no port: any other Android app can reach a loopback
  port. The rules are the desktop page server's (`PageFiles.kt` follows `serve.rs`): GET and HEAD only, only files under
  the app's version folder, a file missing at the root is looked up in the entry page's folder, and each answer carries
  the same Content-Security-Policy, `nosniff` and `no-store`. `os_app_page` gives the shell those files (`files`:
  `root`, `pageDir`, `csp`); the hub opens a slot only when the root is inside the OS's private folder and the origin
  is the app's own. `http`, not `https`: a `.localhost` origin is a secure context either way, and an `https` page
  could not reach a daemon at a plain `ws://` address (mixed content). The desktop keeps its loopback page server.
- Back in an app returns to the shell's desktop; the app keeps running in its slot. A taskbar tap shows it again (same
  process, same state). Each slot is also its own entry in Android's Recents.
- How many slots may be in use at once is the setting `inUse` (shared preferences `bos_slots`), 1 to 4; default 4. The person sets
  it in Start > Settings, "Apps open at once" (AND-6); apps already open keep their slots.
- A slot that Android ends (crash, memory) or that is reused stays on the taskbar, marked "ended"; a tap opens the app fresh (AND-6).
- The sign-in window (`os_login_window_open`, permission `login.window`) is a sign-in view over the app's own slot: every web
  address it reaches is sent to the app's page (`os_login_url`), and back or `os_login_window_close` ends it (`os_login_closed`).
  It runs in the app's slot process, so it shares that app's web storage (the desktop gives it a separate data folder).
  When all are busy, the slot used longest ago is closed and the shell says so plainly ("All 2 app slots were in use,
  so Slot A (the one used longest ago) was closed to open Slot C."). A slot that Android ends (crash, low memory)
  is reported the same way ("... was ended by Android").
- Needs Android 9 (API 28, `minSdk 28`) and a WebView with the message-listener feature.

## Build it

The Android project is generated by Tauri under `crates/battalion-android/gen/android`. The build is debug-signed only
today, with the test package id `org.battalionos.os.devtest`; the release id is not decided yet.

> Android: in progress. The Armory on the phone reading the real `https` catalog and feeds and installing, updating and
> removing apps there is being finished (AND-2); this page describes what is in `battalion-os` today.

# Apps on BattalionOS

An **app** is what BattalionOS runs. This page says what an app is, what it is made of, who manages it, and where
the other kind of extension (a plugin) fits. The chain is: **BattalionOS > apps > each app's plugins.**

The app pages:

| Page | What it covers |
|---|---|
| [overview.md](overview.md) (this page) | What an app is, its two parts, who manages it |
| [create.md](create.md) | `app.json`, every field; developing with live reload |
| [package-and-sign.md](package-and-sign.md) | The `.bosapp` package, the author key, the signature, the feed |
| [armory.md](armory.md) | The Armory: catalogs, feeds, install, update, rollback, remove |
| [android.md](android.md) | BattalionOS on Android: page-only apps and what it refuses |
| [../app-abi.md](../app-abi.md) | The C ABI between the OS's worker and an app's library |
| [../commands.md](../commands.md) | The OS window's commands and events, the frame an app page runs in |

## What an app is

An app is a BattalionOS app. It is not an exe, and Windows or any other platform never starts it. BattalionOS
installs it, starts it, shows it, updates it, stops it and removes it, without restarting itself. It opens inside the
BattalionOS window and can be popped out. It has its own data folder.

An app has up to two parts, both named in its manifest, `app.json` ([create.md](create.md)):

- **A page** (`ui`): web files in the package. The OS shows them in a frame in its window.
- **A process part** (`process.<platform>`): a **library** per platform (`.dll` on Windows, `.dylib` on macOS, `.so`
  on Linux), never a program. The package check refuses `process.<platform>.exec`. BattalionOS starts its own
  worker (its own program with an internal argument), the worker loads the library and calls `run` on the app's own
  thread. The worker is the OS's child, in a Job Object (Windows) or its own process group, and the OS holds its stdin
  (end of file = stop). The C ABI is in [app-abi.md](../app-abi.md).

An app needs at least one of the two for the platform it runs on; with neither, the manifest is refused ("app has
neither a process part for this platform nor a UI part").

**A page-only app** (a `ui` and no process part for this platform) runs on the page runner: it goes through the same
states as any app (starting, running, stopped), with no worker, and opens as its page. That is how every app runs on
Android ([android.md](android.md)), and it works on the desktop too.

Helper programs inside the package that the app itself runs (a CLI, `cli` in `app.json`) are allowed; the OS never
starts them.

BattalionOS manages an app with its own parts and nothing else:

- `battalion-app-api` (Apache-2.0): the contract an app and the OS speak: `app.json`, the catalog and feed formats,
  the ABI.
- `battalion-host` (MPL-2.0): the app manager: store, supervisor, page runner, and the Armory engine (catalog,
  package check, install, update, rollback, remove).
- `battalion-desktop` (MPL-2.0): the BattalionOS window on Windows. `battalion-android` (MPL-2.0) is the same shell
  as an Android app.

The OS names no app. In its code and docs an app is only "an app".

## Who manages it

BattalionOS, always. It creates the app's own thread, runs the app on it and ends it. It installs, updates and
removes apps through the Armory ([armory.md](armory.md)), the OS's one updater, without restarting itself. No product
built on BattalionOS installs, starts, lists or updates an app for it, and an app is never run through a plugin host.

## Where plugins fit

An app is like a plugin in that both extend something, and the OS toward its apps behaves as an app does toward its
plugins (installs, updates and removes them without restarting itself), but the two are never the same thing. A
**plugin** belongs to an app. Any app may have plugins, and only that app manages them: it installs, loads, lists and
removes its own plugins, with its own manifest and its own host. BattalionOS does not know plugins exist and never
manages them; nothing is shared between the OS's app manager and an app's plugin manager.

On threads: by default a plugin runs on its app's thread; it may have its own where needed. An app always has its own,
never shared with the OS or with another app.

| | App | Plugin |
|---|---|---|
| Owner | BattalionOS | The app it extends |
| Manager | The BattalionOS app manager (`battalion-host`, the Armory) | That app's own plugin manager |
| Manifest | `app.json` at the package root | `module.json` at the package root |
| Package | `.bosapp` | The app's own package |
| Where it is listed | The BattalionOS apps page and the Armory catalog | That app's own Plugins screen only |
| Who starts it | BattalionOS, on the app's own thread | The app, inside itself |

## How the OS keeps the line

- A package whose root manifest is `module.json` and not `app.json` is refused by the package check, with the
  message "this package is not an app". Nothing is installed.
- A catalog entry that says it is another kind (`kind` is set and is not `app`) is skipped and reported in the
  listing's rejected entries. The other entries are still listed.
- What an app keeps inside its own package, a folder named for plugins included, is the app's own. The OS does
  not read it, list it or count it.
- A guard test (`crates/battalion-host/tests/line.rs`) fails if an OS crate depends on a plugin host, SDK or WIT
  crate, or if OS source carries the word (a type, field, command, route, event or UI text). This page is the
  only place in the OS's docs where the word may appear.

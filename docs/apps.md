# Apps

An **app** is what BattalionOS runs. This page says what an app is, who manages it, and what it is not.
It is the one page where the other kind of extension is named, so the line between the two stays sharp.

The chain is: **BattalionOS > apps > each app's plugins.**

## What an app is

An app is a BattalionOS app. It is not an exe, and Windows or any other platform never starts it. It runs on its own
thread, created, managed and ended by the OS, never shared with the OS or with another app. It opens inside the
BattalionOS window and can be popped out. It has its own data folder. BattalionOS installs it, starts it, shows it,
updates it, stops it and removes it, without restarting itself. It does this with its own parts and nothing else:

- `battalion-app-api`: the contract an app and the OS speak (`app.json`, the catalog and feed formats).
- `battalion-host`: the app manager (store, supervisor, Armory engine: catalog, package check, install, update, rollback, remove).
- `battalion-desktop`: the BattalionOS window.

An app ships a **library** per target (`.dll`, `.dylib`, `.so`), never a program: `process.<target>.library` in
`app.json`, and the package check refuses `process.<target>.exec`. BattalionOS starts its own worker (its own program
with an internal argument, before any window code), the worker loads the library and calls `run` on the app's thread;
the worker is the OS's child, in a Job Object (Windows) or its own process group, and the OS holds its stdin (end of
file = stop). The C ABI is in [app-abi.md](app-abi.md). Helper programs inside the package that the app itself runs
(a CLI) are allowed; the OS never starts them.

The OS names no app. In its code and docs an app is only "an app".

## Who manages it

BattalionOS, always. It creates the app's own thread, runs the app on it and ends it. It installs, updates and
removes apps without restarting itself. No product built on BattalionOS installs, starts, lists or updates an app
for it, and an app is never run through a plugin host.

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
  only place the word may appear.

Developing an app without packaging it: [develop-an-app.md](develop-an-app.md).

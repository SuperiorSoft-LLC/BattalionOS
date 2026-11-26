# BattalionOS

The core and shell that Infantry AI runs on: an OS that installs, runs, updates and removes **apps**, on Windows and as
an Android app. Work happens on the `battalion-os` branch.

## Docs

| Page | For |
|---|---|
| [docs/apps/overview.md](docs/apps/overview.md) | What an app is and who manages it |
| [docs/apps/create.md](docs/apps/create.md) | `app.json`, every field; developing an app with live reload (`scripts/bos-dev.ps1`) |
| [docs/apps/package-and-sign.md](docs/apps/package-and-sign.md) | The `.bosapp` package, the author key, the feed |
| [docs/apps/armory.md](docs/apps/armory.md) | The Armory (the OS's app manager and updater): catalogs, install, update, rollback, remove |
| [docs/apps/android.md](docs/apps/android.md) | BattalionOS on Android: page-only apps |
| [docs/app-abi.md](docs/app-abi.md) | The C ABI of an app's library |
| [docs/commands.md](docs/commands.md) | The OS window's commands, events and the frame an app page runs in |
| [packaging/README.md](packaging/README.md) | The Windows installer (MSI) and first run |

## Layout

| Part | Path | Licence |
|---|---|---|
| OS host (app manager, Armory, page runner) | `crates/battalion-host` | MPL-2.0 |
| OS desktop shell and its UI | `crates/battalion-desktop` | MPL-2.0 |
| OS as an Android app | `crates/battalion-android` | MPL-2.0 |
| App contract (`app.json`, catalog, feed, ABI) | `crates/battalion-app-api` | Apache-2.0 |
| Demo app and page-only app fixtures | `fixtures/demo-app`, `fixtures/page-app` | Apache-2.0 |

Each crate and `fixtures/demo-app` hold their own `LICENSE`; the full texts are also `LICENSE-MPL-2.0` and `LICENSE-Apache-2.0` at the root. Source files carry an `SPDX-License-Identifier` header.

# Apps

An **app** is what BattalionOS runs. This page says what an app is, who manages it, and what it is not.
It is the one page where the other kind of extension is named, so the line between the two stays sharp.

## What an app is

An app is a program with its own process, its own window and its own data folder. BattalionOS installs it,
starts it, shows it, updates it, stops it and removes it. It does this with its own parts and nothing else:

- `battalion-app-api`: the contract an app and the OS speak (`app.json`, the catalog and feed formats).
- `battalion-host`: the app manager (store, supervisor, Armory engine: catalog, package check, install, update, rollback, remove).
- `battalion-desktop`: the BattalionOS window.

The OS names no app. In its code and docs an app is only "an app".

## Who manages it

BattalionOS, always. No product built on BattalionOS installs, starts, lists or updates an app for it, and an
app is never run through a plugin host.

## What it is not

A **plugin** belongs to one product, inside that product. The product alone manages its plugins: installs,
loads, lists and removes them, with its own manifest and its own host. BattalionOS does not know plugins exist.
The two behave alike in places (both are installable, both have a manifest), but they are never treated as the
same thing and nothing is shared between the two managers.

| | App | Plugin |
|---|---|---|
| Owner | BattalionOS | One product built on BattalionOS |
| Manager | The BattalionOS app manager (`battalion-host`, the Armory) | That product's own plugin manager |
| Manifest | `app.json` at the package root | `module.json` at the package root |
| Package | `.bosapp` | The product's own package |
| Where it is listed | The BattalionOS apps page and the Armory catalog | The product's own Plugins screen only |
| Who starts it | The BattalionOS supervisor, as its own process | The product, inside itself |

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

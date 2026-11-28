# @battalionos/app-api

The typed client a BattalionOS app's page uses (`packages/app-api`, Apache-2.0). One promise per OS call, typed errors, and a
local backend so a page runs in a plain browser without the OS. No framework dependency. Not on npm yet: use it from this
repository (the template links it). A walkthrough: [Make your own app](make-your-own-app.md).

## Connect

```ts
import { connect } from "@battalionos/app-api";
const bos = connect();                // { backend: "auto" | "os" | "local", timeoutMs = 30000, local: LocalOptions }
bos.backend;                          // "os" in an OS frame, "local" in a plain browser
```

Under the OS the page talks over the frame bridge: it posts `{bos: 1, id, cmd, args}` to its parent and gets
`{bos: 1, id, ok, result | error}`; events come as `{bos: 1, event, ...}` ([commands.md](../commands.md#the-page-in-a-frame-bos-17)).
The shell answers only the frame's own app, so a page never acts for another app. `createBos(transport)` takes any
`Transport` (`send`, `listen`) for another runtime.

## Calls

| Client | OS call | Permission |
|---|---|---|
| `bos.fs.list(path?, {cursor?, limit?})`, `bos.fs.listAll(path?)` | `os_fs_list` (listAll follows `cursor`) | `files.read` |
| `bos.fs.stat(path?)` | `os_fs_stat` | `files.read` |
| `bos.fs.read(path, {offset?, length?, as?})`, `readText(path)`, `readBytes(path)` | `os_fs_read` (readText/readBytes read every chunk) | `files.read` |
| `bos.fs.watch(path, fn)` -> `stop()` | `os_fs_watch`, `os_fs_changed` events, `os_fs_unwatch` | `files.read` |
| `bos.fs.write(path, data, {as?, overwrite?})`, `writeText`, `writeBytes` | `os_fs_write` | `files.write` |
| `bos.fs.mkdir`, `rename`, `copy`, `trash`, `delete(path, {recursive?})` | `os_fs_mkdir`, `os_fs_rename`, `os_fs_copy`, `os_fs_trash`, `os_fs_delete` | `files.write` |
| `bos.store.get(key)`, `set(key, value)`, `delete(key)`, `list(prefix?)` | `os_store_*` (the app's own store) | none |
| `bos.notify(title, text?, {tag?, route?})` | `os_notify` | `notifications` |
| `bos.badge(count?, text?)` | `os_badge` | none |
| `bos.windows.open/close/minimize/restore/maximize/focus/snap/set/list`, `onEvent` | `os_win_*`, `os_win` events | `window` |
| `bos.pickFolder(title?)`, `bos.pickFile(title?)` | `os_pick_folder`, `os_pick_file` | `files.user` |
| `bos.openExternal(url)` | `os_open_external` | `network` |
| `bos.call(cmd, args)`, `bos.onEvent(fn)` | any call / every event (a newer call this client does not wrap) | |

Paths are relative to the OS root folder, `/`-separated; `..`, absolute paths and links out are refused.
The full argument and result tables: [commands.md](../commands.md#file-calls-of-a-page-in-a-frame-fs-api-1).

## Errors

Every rejected call throws a `BosError` (`code`, `call`, `message`). The OS ends a refusal with its code (`... [not_found]`);
the client maps it:

| Class | `code` | When |
|---|---|---|
| `PermissionError` (`permission`) | `permission` | `app.json` lacks the permission the call needs |
| `LeavesRootError` | `leaves_root` | a path out of the OS root |
| `NotFoundError` | `not_found` | no such file or folder |
| `ExistsError` | `exists` | the name is taken |
| `TooLargeError` | `too_large` | over a limit (`fs.writeMaxBytes`, `store.maxBytes`, ...) |
| `BadArgsError` | `bad_args` | wrong or missing arguments |
| `NotAvailableError` | `not_available` | not served on this device, this OS, or the local backend |
| `TimeoutError` | `timeout` | no answer in `timeoutMs` |
| `BosError` | the OS's code | any other (`not_a_folder`, `not_text`, `not_empty`, `is_root`, `io`, ...) |

## Types and schemas

The types are generated from the contract's JSON schemas, `crates/battalion-app-api/schemas/fs.schema.json`,
`store.schema.json` and `window.schema.json` (the WM-5 pattern): `FsEntry`, `FsList`, `FsStat`, `FsRead`, `FsChangedEvent`,
`StoreFile`, `WindowSpec`, `WinEvent`, ... Other languages generate from the same files. `test/gen.test.ts` fails when a
generated file is stale; regenerate with `UPDATE_GEN=1 pnpm vitest run test/gen.test.ts`. A Rust test
(`crates/battalion-host/tests/appstore.rs`) checks that the schemas name exactly the calls and codes the host serves.

## The local backend

`connect()` in a plain browser, or `createBos(localTransport(opts))`: an in-memory file tree (`files: {path: text}`) and
store (kept in `storage`, e.g. `localStorage`, under `storageKey`), with the OS's result shapes and refusal codes. It is for
development: smaller limits, no links, and the native calls (pickers, windows, open external) are `NotAvailableError`.

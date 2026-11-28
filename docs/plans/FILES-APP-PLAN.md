# FILES-APP plan: general app APIs, and a Files app as the first Armory test app

Status: plan only (FILES-APP-0, 2026-11-28). No code, repo, key or release is made by this document. Where this page and
the code disagree later, the code wins.

**The owner's point.** BattalionOS offers general, documented, permission-gated building blocks that any developer can
use to make their own apps (their own file manager, their own editor). Nothing is special-cased for our Files app. The
Files app is the first consumer: it uses the same public messages, the same client library and the same Armory path as a
stranger's app would. If the Files app needs something a stranger cannot have, the plan is wrong.

## 0. What exists today (verified 2026-11-28 on `battalion-os` acea670)

| Piece | Where | State |
|---|---|---|
| Manifest `app.json`, permissions list | `crates/battalion-app-api/src/manifest.rs` (`PERMISSIONS`), [apps/create.md](../apps/create.md) | `process.spawn`, `network`, `files.user`, `window`, `login.window`, `notifications`, `scan.code`. An unknown value is refused. |
| Page bridge | [commands.md](../commands.md#the-page-in-a-frame-bos-17) | `postMessage({bos: 1, id, cmd, args})`; the app id is the frame's, never an argument; the permission is read from the **installed** `app.json` at every call. |
| File calls | [commands.md](../commands.md#native-calls-of-a-page-in-a-frame-bos-25) | Only `os_pick_folder` / `os_pick_file` (`files.user`). They return a **path string** (desktop) or a `content://` address (Android). There is no list/read/write/rename/delete. A directory app cannot work. |
| Clipboard | same section | The frame's own (`allow="clipboard-read; clipboard-write"`). Works for text inside one page. |
| Window calls + a TS client | `kit/lib/windows` (WM-5), `battalion-app-api::windows`, `schemas/window.schema.json` | The pattern to copy: Rust types -> JSON schema -> generated TS types -> a client with an OS backend and a local backend. |
| Notifications, code scan | `os_notify`, `os_scan_code` | Exist. |
| Armory | [apps/armory.md](../apps/armory.md), [apps/package-and-sign.md](../apps/package-and-sign.md) | Catalog v2, feeds, minisign with a pinned key, install/update/rollback/remove, local repos. |
| Public catalog | `SuperiorSoft-LLC/battalionOS-apps` `catalog.json` (schema 2) | One app listed. README "Add your app". |
| A packaging tool | none in BattalionOS; the Infantry AI repo's `scripts/bos-package.py` packs one app | "Any tool that produces those files works." |
| A JS client package for apps | none published | The kit is the OS's own UI code, not a package a stranger installs. |

Overlap check: no done or archived ticket, commit or code adds a file-system API, an app SDK package, "open with",
drag and drop between apps or an app settings store. `git log --grep` on BattalionOS shows BOS-25 (pickers), WM-5
(window client) and ATTACH-AND-1 (`<input type=file>` on Android) only; this plan extends them and changes none of them.

## 1. The general app API surface (additive; old apps unchanged)

Rules every new call follows (they are today's rules, kept):

- Same message form and identity rule as BOS-25. The app id is the frame's. Nothing in `args` names an app.
- Each call needs one permission from the installed `app.json`, read at every call; refused with the permission's name.
- New permissions are added to `PERMISSIONS` (additive). An app that does not declare them sees no change.
- Every limit is an `os.json` setting with a default (the `armory.*` pattern), never a constant.
- The shell serves each page call with a main-only `os_frame_*` command, as today; the work is done in
  `battalion-host` (a new `fs` module), not in the desktop crate, so the Android build shares the rules.

### 1a. Files

#### Options for the scope (the security decision)

| Option | What the app reaches | For | Against |
|---|---|---|---|
| A. Whole disk behind `files.user` | Any path | Simplest; a file manager "just works" | One permission = the whole disk, including other apps' data and `~/.ssh`. A stranger's app with a bug or bad intent reads everything. Cannot be narrowed later without breaking apps. **Rejected.** |
| B. Fixed user folders (Documents, Downloads, Pictures, Desktop) | Those folders | No dialog | Arbitrary choice of folders; still broad; does not map to Android (no paths). |
| **C. Grants: roots the person picks** | Only folders/files the person chose in an OS picker for **this** app, remembered per app | Least privilege; the person sees and controls it; maps 1:1 to Android's Storage Access Framework (persisted URI permissions); revocable | One picker click before first use; grant UI needed |

**Chosen: C, with a grant handle, not a path.** It is the only option where "an app never reaches beyond what the user
granted" is a property of the API and not of the app's good behaviour. It is also the option that is cheapest to
loosen later (a new permission could add B for apps that ask) and the most expensive to tighten later, so we start
tight.

#### Expensive to reverse (say it plainly)

1. **Handles vs paths in the API.** If calls take raw OS paths, every app hard-codes `C:\...` and Android cannot work.
   Once strangers ship apps against paths, switching to handles breaks them all. So: calls take
   `{root, path}` where `root` is an opaque grant id and `path` is a relative, `/`-separated path inside it. The OS
   path is never needed by the app (it may be shown, see `stat.display`).
2. **What `files.user` means.** Today it means "may open the picker". We keep that meaning exactly and add new
   permissions for reading and writing, so an existing app's grant never silently widens.
3. **The grant store format** (`<home>/state/grants/<app>.json`): it becomes part of what a backup or a reinstall
   carries. Versioned from day one (`{"schema": 1, ...}`).

#### Permissions (additive)

| Permission | Allows | Notes |
|---|---|---|
| `files.user` (unchanged) | `os_pick_file`, `os_pick_folder` (path result, as today) | Old apps unchanged. |
| `files.read` (new) | Grant pickers (`os_fs_grant`) for read, and `list`, `stat`, `read`, `watch` inside a read grant | |
| `files.write` (new) | Grants for write, and `write`, `mkdir`, `rename`, `copy`, `trash`, `delete` inside a write grant | Implies nothing outside a grant. |

Not added: a `files.browse` whole-disk permission. If a real need appears (a backup app), it is a separate ticket
with its own consent screen.

#### Grants

- `os_fs_grant {kind: "folder"|"file"|"save", mode: "read"|"readwrite", title?, suggestedName?, types?}` opens the
  OS picker (the same `rfd` / Android SAF picker as today) and returns
  `{root, name, kind, mode, display}` or `null` on cancel. `root` is an opaque id (`g-<random>`), stable for this app.
- The grant is stored in `<home>/state/grants/<app>.json`: `{schema: 1, grants: [{root, kind, mode, osPath | uri,
  created, lastUsed}]}`. Remembered across restarts and updates; deleted when the app is removed with purge.
- `os_fs_grants` lists the app's grants; `os_fs_revoke {root}` drops one. The OS's own app page in the Armory lists an
  app's grants with a Revoke button (the person, not the app, has the last word).
- A grant on Android is a persisted SAF URI permission (`takePersistableUriPermission`); revoke releases it.
- `save` grants a single new file the person named in a Save dialog (the editor's "Save as").

#### Calls (all `{root, path}`; `path` is relative, `""` = the root itself)

| `cmd` | Args | Permission | Result |
|---|---|---|---|
| `os_fs_list` | `root, path, cursor?, limit?` | files.read | `{entries: [{name, kind: file\|dir\|link, size, modified}], cursor}` (paged; `limit` capped by `fs.listMax`) |
| `os_fs_stat` | `root, path` | files.read | `{name, kind, size, modified, created?, readonly, display}` (`display`: a human path for the title bar, never to be fed back) |
| `os_fs_read` | `root, path, offset?, length?, as?: "text"\|"base64"` | files.read | `{data, eof, size}` (chunked; one answer capped by `fs.readChunkBytes`) |
| `os_fs_write` | `root, path, data, as?, mode: "create"\|"replace"\|"append", ifMatch?` | files.write | `{size, modified, etag}`. `replace` is atomic (temp file in the same folder, then rename). `ifMatch` (the etag from `stat`/`read`) refuses a write over a file changed elsewhere: the editor's "file changed on disk" guard. |
| `os_fs_mkdir` | `root, path` | files.write | `{}` |
| `os_fs_rename` | `root, path, to` | files.write | `{}`; `to` in the same grant |
| `os_fs_copy` | `root, path, toRoot, to` | files.read on source, files.write on target | `{}`; across grants of the same app only; progress for a large copy comes to the frame as `{bos: 1, event: "os_fs_progress", ...}` |
| `os_fs_trash` | `root, path` | files.write | `{}`; the system recycle bin (Windows) / delete with confirm on Android |
| `os_fs_delete` | `root, path` | files.write | `{}`; permanent. The OS asks the person once per call for a folder (an OS dialog the app cannot draw or skip) |
| `os_fs_watch` / `os_fs_unwatch` | `root, path` | files.read | a watch id; events `{bos: 1, event: "os_fs_changed", watch, path, change}` to that frame only; at most `fs.watchMax` per app; debounced |

Large data goes in chunks over the existing bridge. A later option (only if measured too slow) is a per-app
loopback file route on the app's page server with a one-use token; not in the first tickets.

#### Path rules (no escaping a grant)

Checked in `battalion-host::fs`, the one place, with tests for each:

1. `path` is relative, `/`-separated; refused: absolute, a drive (`:`), `\`, `..`, `.` segments, NUL, control
   characters, empty segments, Windows reserved names (`CON`, `NUL`, `COM1`...), trailing dot or space. Same spirit as
   the page server's rules in commands.md "Origin".
2. The resolved real path (after following links) must stay under the grant's real root, checked **at the open**
   (open the file, then check the opened handle's final path, `GetFinalPathNameByHandle` on Windows), so a link swapped
   in between check and use cannot win (TOCTOU).
3. **Symlinks and junctions:** listed as `kind: "link"`; following one is allowed only when its target is inside the
   same grant; otherwise the call is refused ("leaves the grant"). Never create links (no API for it).
4. A file grant reaches that one file only; a folder grant its tree.
5. The OS's own home (`BATTALION_HOME`), other apps' data folders and the webview folders are refused even when a
   person picks a folder containing them (a grant whose root contains the OS home is refused at the picker with a
   plain sentence).
6. Sizes: `fs.readChunkBytes` (default 4 MiB), `fs.writeMaxBytes` per call (16 MiB), `fs.listMax` (1000),
   `fs.watchMax` (32). Rate limit per app (`fs.callsPerSecond`, 200).
7. Android: `root` maps to a SAF tree/document URI; `path` is resolved by walking `DocumentFile` children by name. The
   same rules, the same errors. `trash` = delete (SAF has no bin) after an OS confirm.

#### How an editor opens and saves

- **Open:** `os_fs_grant {kind: "file", mode: "readwrite", types: [".md", ".docx"]}` -> `{root}`; `os_fs_read
  {root, path: ""}` -> content + etag. The grant is remembered, so "recent files" can reopen it next time without a
  dialog.
- **Save:** `os_fs_write {root, path: "", data, mode: "replace", ifMatch: etag}`. A conflict answers
  `{error: "changed", etag}` and the editor asks the person.
- **Save as:** `os_fs_grant {kind: "save", suggestedName}` -> a new `{root}`, then write.
- **Opened from the Files app ("open with"):** see 1b; the editor receives a grant, never a path.

### 1b. The other shared building blocks, ranked by need

| Rank | Block | Exists? | Plan |
|---|---|---|---|
| 1 | **Open / save dialogs** | Partly (`os_pick_*` return paths) | Now: they are `os_fs_grant` (`file`, `folder`, `save`). |
| 2 | **Per-app settings/state store** | No API (a page has its own `localStorage` per webview folder; a process part has its state dir) | Now: `os_store_get/set/delete/list {key}` (permission none; scoped to the app; `store.maxBytes` 1 MiB) in `<home>/data/<app>/store.json`. Survives updates, kept on remove like the state folder. Cheap and every app needs it. |
| 3 | **Recent files** | No | Now, built on the store + grants: `os_fs_recent` returns the app's grants sorted by `lastUsed`. No new storage. |
| 4 | **Open with (file-type associations)** | No | Later (ticket 5). `app.json` gains `opens: [{types: [".md"], verb: "edit"}]`. `os_open_with {root, path}` (files.read) asks the OS for apps that declared the type, lets the **person** choose, and the OS hands the chosen app a **new grant** for that one file (read or readwrite, as the target declared) through `{bos: 1, event: "os_open_request", root}` and `os_take_open_request` (the name already exists for `--open`). The opener never learns the other app's identity beyond its name; the target never gets the opener's folder. |
| 5 | **Clipboard** | Text yes (the frame's own) | Later: files on the clipboard = a grant; only if a real app needs it. Text and images keep using the browser API. |
| 6 | **Drag and drop between apps** | Within one page only | Later: the shell is the only thing that sees both frames; a drop of files from app A onto app B becomes a grant for B, like "open with". After open-with, since it reuses its grant hand-off. |
| 7 | **Apps as tools (call another app's tools; tools for agents)** | No | Later, own plan: `app.json` `tools: [{name, schema}]`, `os_tool_call {app, tool, args}` with a person-approved per-pair grant. It is the agent hook the owner wants, but it is a contract between strangers' apps, so it needs its own design and is not on the Files app's path. |

### 1c. A client library, types, docs and a template

Options: (a) apps copy the `postMessage` code from the docs; (b) a small published package; (c) generated clients in
several languages. **Chosen: (b) plus the schema**, the WM-5 pattern:

- **Contract types:** Rust types in `battalion-app-api::fs` and `::store`, JSON schemas in `crates/battalion-app-api/schemas/`
  (`fs.schema.json`, `store.schema.json`). The schema is the contract; the TS types are generated from it, so other
  languages can generate their own.
- **Package `@battalionos/app-api`** (Apache-2.0, npm, in the BattalionOS repo under `packages/app-api`): a promise
  client over the bridge (`bos.fs.grant()`, `bos.fs.list()`, `bos.store.get()`, `bos.windows.*` moved from the kit,
  `bos.notify()`), typed errors (`PermissionError`, `LeavesGrantError`, `ChangedError`), chunked read/write helpers,
  and a **local backend** for running the page in a plain browser during development (File System Access API or an
  in-memory tree), so a stranger can build without the OS running. No framework dependency.
- **Docs:** `docs/apps/files.md` (the calls, the grant model, the rules, the errors, Android), `docs/apps/app-api.md`
  (the package), and `create.md` gains the new permissions. commands.md gets the call tables (frozen-v1 rule: add,
  never rename).
- **Template:** `templates/page-app` in the BattalionOS repo: Vite + TypeScript (React optional), `app.json`, the
  package, `scripts/package.mjs` (zip -> `.bosapp`, sign with minisign, write `feed.json` + `catalog-entry.json`) and a
  GitHub workflow. This is the generic packaging tool package-and-sign.md says is missing; it is Node, so a stranger
  needs no Python and no Infantry AI repo. The Files repo is created **from** this template.

## 2. The new app repo

- **Name:** `SuperiorSoft-LLC/battalionos-files` (app id `files`; display name "Files"). Apache-2.0, public.
- **Stack:** page-only app (no process part, so the same package runs on Windows and Android), React + TypeScript +
  Vite, `@battalionos/app-api`. No native code, no `init`.
- **app.json:**

```json
{
  "manifest": 1,
  "id": "files",
  "name": "Files",
  "version": "0.1.0",
  "publisher": "SuperiorSoft LLC",
  "license": "Apache-2.0",
  "icon": "icon.png",
  "contract": { "min": 1, "max": 1 },
  "minOs": "<the first OS version with files.read>",
  "ui": { "entry": "ui/index.html", "window": { "width": 1000, "height": 700 } },
  "state": { "schema": 1 },
  "permissions": ["files.read", "files.write", "window", "notifications"]
}
```

  Note: `files.user` is not needed (it only covers the old path pickers). `opens` is added when open-with lands.
- **Dev live reload:** `pnpm dev` (Vite on `127.0.0.1:5173`), then from a BattalionOS checkout
  `scripts/bos-dev.ps1 -App <repo>\dist-app -AppUrl http://127.0.0.1:5173` (the existing developer mode, nothing new).
  `pnpm dev:browser` runs it against the local backend with no OS.
- **CI** (`.github/workflows/release.yml`, from the template): on a tag `v*` -> `pnpm build` -> `package.mjs` writes
  `files-<v>-windows-x86_64.bosapp` and `files-<v>-android-aarch64.bosapp` (identical page content; two feed entries) ->
  sign with the secret key -> verify the signature against `public.key` in the repo -> GitHub release `v<v>` with the
  packages, `.minisig`, `feed.json`, `catalog-entry.json` -> move the `prerelease` channel release (`channel-prerelease`,
  prerelease, never latest) to the new `feed.json`. `stable` is moved by a manual `workflow_dispatch` after the test in
  section 4. A run without the secret signs with a throwaway key and marks the release "not trusted" (the Infantry AI
  pattern).
- **Public example:** the README is written for a stranger ("how this app was made"; every OS call it uses links to
  the docs); no step uses anything outside the public docs, the package and the template.

## 3. App signing key

- **What:** one minisign key per app author (here: SuperiorSoft for the Files app). Not the OS updater key, not the
  Infantry AI app key: a separate key, so a leak of one cannot sign the others.
- **Where it is made:** on the owner's PC, by the owner. One script, `scripts/new-app-key.ps1` in the template, runs
  `minisign -G` into a folder the owner chooses (the owner's own key folder; agents never read it, nor the owner's existing
  Infantry and BattalionOS key folders in the home folder), then pipes the secret key straight into
  `gh secret set BOS_APP_SIGNING_KEY -R SuperiorSoft-LLC/battalionos-files` and the password into
  `BOS_APP_SIGNING_KEY_PASSWORD`, and writes only `public.key` into the repo.
- **GitHub secrets (repo `battalionos-files`):** `BOS_APP_SIGNING_KEY`, `BOS_APP_SIGNING_KEY_PASSWORD`. The template
  uses these generic names, so a stranger's repo uses the same names.
- **Backup:** the owner keeps a copy offline. Losing it means installs cannot be updated, only removed and installed
  again (the pin, package-and-sign.md).
- **Owner steps (exactly):** (1) create the repo `SuperiorSoft-LLC/battalionos-files` from the template (or approve an
  agent doing it); (2) run `scripts/new-app-key.ps1` once; (3) approve the catalog PR in `battalionOS-apps`;
  (4) the install / update / rollback check on their own BattalionOS (section 4), since agents never touch the
  running OS.

## 4. The Armory test

### The catalog entry (`battalionOS-apps/catalog.json`, `apps[]`)

```json
{
  "id": "files",
  "name": "Files",
  "publisher": "SuperiorSoft LLC",
  "summary": "Browse, copy, rename and open your files.",
  "license": "Apache-2.0",
  "icon": "https://raw.githubusercontent.com/SuperiorSoft-LLC/battalionos-files/main/icon.png",
  "homepage": "https://github.com/SuperiorSoft-LLC/battalionos-files",
  "publicKey": "<the base64 line of public.key>",
  "contract": { "min": 1, "max": 1 },
  "permissions": ["files.read", "files.write", "window", "notifications"],
  "channels": {
    "stable": "https://github.com/SuperiorSoft-LLC/battalionos-files/releases/download/channel-stable/feed.json",
    "prerelease": "https://github.com/SuperiorSoft-LLC/battalionos-files/releases/download/channel-prerelease/feed.json"
  }
}
```

### The PR flow a third party follows (the README "Add your app", extended)

1. Build from the template; tag a release; CI writes `catalog-entry.json`.
2. Fork `battalionOS-apps`, paste the entry into `apps[]`, open a PR.
3. A catalog CI check (new, ticket 6) runs on the PR: schema 2 valid; id unique; `publicKey` parses; every channel feed
   downloads; each platform package's size/sha256 match and the minisign signature verifies against `publicKey`;
   `app.json` id/version match; the declared `permissions` equal the package's `app.json` permissions. The same Rust
   code the OS uses (`battalion-app-api::catalog`), run as a small CLI, so the check cannot drift from the OS.
4. A maintainer reviews the permissions and merges. Our Files app goes through exactly these steps, as a PR, not a
   direct push.

### Install, update, rollback on the owner's BattalionOS (owner's hands; agents prepare)

Before the owner: the same steps pass in Windows Sandbox (`packaging/make-sandbox-kit.ps1`, `file:` catalog) and on a
test OS with its own `BATTALION_HOME` (never the owner's).

1. **Install:** Armory -> Files -> Install (stable). Expect: progress `download, verify, unpack, place`; the icon on
   the desktop; opening it shows an empty state "Pick a folder"; picking one lists it.
2. **Grant check:** pick `Documents`. In the app try a path with `..` from the dev console (`bos.fs.list({root, path:
   "../"})`): expect "leaves the grant". The Armory app page lists the grant; Revoke it; the app's next call is refused.
3. **Update:** CI tags `0.1.1` (a visible change, e.g. version in the About box) and moves `prerelease`; switch the
   app's channel to `prerelease` (`os_set_app_channel`, existing); Update. Expect the frame to reopen on 0.1.1 and the
   grant still there.
4. **Rollback:** Armory -> Files -> Roll back. Expect 0.1.0 again, the grant still there.
5. **Bad signature:** a `0.1.2` feed signed with a throwaway key (a test channel `test-badkey`) is refused ("other key").
6. **Phone:** install the same catalog entry on Android; pick a folder (SAF); list and open a file.
7. **Remove:** Remove; the grants file stays unless purged; reinstall finds them.

## 5. Implementation tickets, in order

| # | Ticket | Tests | Risk |
|---|---|---|---|
| 1 | **FS-API-1: grants + read side** in `battalion-host::fs` and the shell: permissions `files.read`/`files.write` (manifest), `os_fs_grant/grants/revoke`, `list`, `stat`, `read`, the path rules, the grant store, settings `fs.*`; Rust types + `fs.schema.json`; commands.md + `docs/apps/files.md`. Desktop only. | Unit: every path rule (each refused form, link out of the grant, link inside, file grant reaches one file, OS home refused, TOCTOU swap test with a junction); permission refused by name; old app with only `files.user` unchanged (BOS-25 tests stay green); `commands_frozen`, `no_source_file_of_the_os_names_an_app`. | **High** (security). Second reviewer on the path code. |
| 2 | **FS-API-2: write side + watch**: `write` (atomic replace, `ifMatch`), `mkdir`, `rename`, `copy`, `trash`, `delete` (OS confirm), `watch`; limits as settings. | Atomic replace leaves the old file on a failed write; `ifMatch` conflict; copy across two grants of one app, refused across apps; trash goes to the bin; watch events reach only the owning frame; rate limit. | High (data loss). Trash before delete in the Files app. |
| 3 | **APP-API-1: `@battalionos/app-api` + store + template**: `packages/app-api` (client, errors, chunk helpers, local backend), `os_store_*`, `os_fs_recent`, `templates/page-app` with `package.mjs`, `new-app-key.ps1`, release workflow; `docs/apps/app-api.md`. | Package unit tests against a fake bridge; schema -> types generation check (types match schema); `package.mjs` output verifies with the OS's own package check (a Rust test reads a template-made `.bosapp`). | Medium (a public contract: naming is expensive to change; review names once). |
| 4 | **FS-API-AND-1: Android backend**: grants as persisted SAF URIs, the same calls through `os_frame_phone_call`, same errors. | Instrumented test on the emulator: grant, list, read, write, revoke; a refused `..`. | Medium (SAF differences: no bin, slow name lookups). |
| 5 | **FILES-APP-1: the Files repo and app** (needs the owner to create the repo and the key): from the template; list/grid view, breadcrumbs, copy/rename/trash, recent; CI release; catalog PR; section 4 checks in a sandbox. | App unit tests on the local backend; CI verifies the signature; sandbox install/update/rollback. | Low for the OS (it is only a consumer). |
| 6 | **ARMORY-CHECK-1 + OPEN-WITH-1** (can split): catalog PR check CLI in `battalionOS-apps` CI; then `opens` in `app.json`, `os_open_with`, the grant hand-off (`os_open_request`). The editor app follows after this. | Catalog check passes the current catalog and fails each broken form; open-with hands the target a one-file grant, never the opener's folder, and only after the person chooses. | Medium. |

Clipboard files, drag and drop between apps and apps-as-tools stay out of these tickets (section 1b, later).

## 6. Decisions to confirm (for the owner)

1. Scope = grants the person picks (option C), no whole-disk permission for now. Alternative: whole disk behind one
   permission (rejected above). Reversible only in the loosening direction.
2. New permissions `files.read` / `files.write`; `files.user` keeps its meaning. Alternative: widen `files.user`
   (rejected: it would silently widen every installed app).
3. The template and package live in the BattalionOS repo (`templates/`, `packages/`). Alternative: separate repos
   (more release plumbing for no gain now).
4. App id `files`, repo `battalionos-files`, separate signing key with secrets `BOS_APP_SIGNING_KEY(_PASSWORD)`.

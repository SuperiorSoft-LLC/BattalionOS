# Make your own app

Anyone can build a BattalionOS app from three public pieces in this repository: the **template**
(`templates/page-app`), the **client package** [`@battalionos/app-api`](app-api.md) (`packages/app-api`) and the **packer**
(`templates/page-app/scripts/package.mjs`). You need Node 22+ and pnpm. No Rust, no Python, no other repository.

This page builds a page app: a web page (React + TypeScript + Vite here, any web stack works) that the OS shows in a
window. The same package runs on Windows and on Android, since it holds no native code.

## 1. Start from the template

```
cp -r templates/page-app ../my-app        # or copy the folder by hand
cd ../my-app
pnpm install
```

Out of this repo, change the package line in `package.json` from `link:../../packages/app-api` to the path of your copy of
`packages/app-api` (the package is not on npm yet).

Edit `app.json` ([every field](create.md#every-field)): a unique `id` (lowercase, digits, `-`), your `name`, `publisher`
and `version`. Ask only for the permissions you use:

| Permission | Gives the page |
|---|---|
| `files.read` | `bos.fs.list/stat/read/watch` under the OS root folder |
| `files.write` | `bos.fs.write/mkdir/rename/copy/trash/delete` under the OS root (it does not grant `files.read`) |
| `notifications` | `bos.notify` |
| `window` | `bos.windows.*` (more windows of your own) |
| `files.user` | `bos.pickFolder/pickFile` |
| `network` | `bos.openExternal` |
| (none) | `bos.store.*`, your app's own key/value store; `bos.badge` |

## 2. Write the page

Every OS call goes through one client:

```ts
import { connect, NotFoundError } from "@battalionos/app-api";

const bos = connect();                         // the OS in a frame; the local backend in a plain browser

const entries = await bos.fs.listAll("notes");  // [{name, kind, size, modified}]
const text = await bos.fs.readText("notes/todo.txt");
await bos.fs.writeText("notes/todo.txt", text + "- one more\n");
await bos.store.set("lastDir", "notes");        // your app's own store, kept across updates
await bos.notify("Saved", "notes/todo.txt");
```

Errors are typed: `PermissionError` (add the permission it names to `app.json`), `LeavesRootError`, `NotFoundError`,
`ExistsError`, `TooLargeError`, `BadArgsError`, `NotAvailableError`, all `BosError` with a stable `code`. The whole API:
[app-api.md](app-api.md).

The template's demo (`src/App.tsx`) lists a folder, opens a text file, edits and saves it, follows changes with
`bos.fs.watch`, and remembers the last folder in `bos.store`. Its components show the loading, empty and error states.

### Open files from other apps

Declare the files your app opens in `app.json`, and the OS offers it in "Open with" (for example from the Files app):

```json
"fileTypes": [{ "label": "Text file", "extensions": ["txt", "md"], "mime": ["text/plain"] }]
```

```ts
bos.onOpenFile(async (path) => {                 // each file handed to this app, also one handed over before the page loaded
  show(path, await bos.fs.readText(path));
});
await bos.openWith("notes/todo.txt");            // hand a file to another app: the person chooses (or the remembered default)
```

Without `files.read` your app can still read and stat each file it is handed (a one-file grant), and nothing else: ask for
`files.read` only if the app browses the root itself, and `files.write` to save. The template declares text files, opens one
it is handed, and has an "Open with…" button. Details: [commands.md](../commands.md#open-with-open-with-1).

## 3. Run it while you work

- **In a plain browser, no OS:** `pnpm dev` and open http://127.0.0.1:5173. `connect()` sees no OS and uses the local
  backend: an in-memory file tree with sample files (`src/bos.ts`) and the store in `localStorage`.
- **In BattalionOS with live reload:** `pnpm build` once (it writes the app folder `dist-app/`: `app.json` + `ui/`), keep
  `pnpm dev` running, then from a BattalionOS checkout:

  ```
  scripts/bos-dev.ps1 -App <your-app>\dist-app -AppUrl http://127.0.0.1:5173
  ```

  The OS loads your page from the Vite server, so a save reloads it in the OS window ([create.md](create.md#develop-with-live-reload)).
- **Tests:** `pnpm test` runs the demo's calls on the local backend and the packer's checks (vitest).

## 4. Package and sign

```
pnpm build
pnpm package -- --base-url https://example.org/my-app/releases/0.1.0/
```

`scripts/package.mjs` writes into `out/`:

| File | What it is |
|---|---|
| `<id>-<version>-windows-x86_64.bosapp`, `...-android-aarch64.bosapp` | the zip of `dist-app/` (the same page for both) |
| `*.bosapp.minisig` | the minisign signature of each package |
| `feed.json` | version, contract, and for each platform `url`, `signature`, `sha256`, `size` |
| `catalog-entry.json` | your app's entry for a catalog: `publicKey`, `channels`, `platforms` |
| `public.key` | the base64 line of your public key |

**The key comes from the environment only.** The script never opens a key file:

| Variable | Value |
|---|---|
| `BOS_APP_SIGNING_KEY` | the text of your minisign secret key (`minisign -G` makes one; keep the file somewhere only you can read) |
| `BOS_APP_SIGNING_KEY_PASSWORD` | its password, when it has one |

In CI, put both in the repository's secrets. Without `BOS_APP_SIGNING_KEY` the script signs with a **throwaway** key and
says so: fine to try an install locally, never to publish. Keep your real key: every update must be signed with the key
the first install pinned ([package-and-sign.md](package-and-sign.md#the-author-key-and-the-signature)).

## 5. Publish

Upload the `out/` files where `--base-url` points (a GitHub release works), then add `catalog-entry.json` to a catalog
(the public one is `SuperiorSoft-LLC/battalionOS-apps`, by pull request) or to a local repo for yourself
([armory.md](armory.md)). The OS checks size, sha256, the signature with your key, the zip and `app.json` before anything
is installed.

## 6. Install and test locally

You do not need to publish anything to try your app in BattalionOS. Everything below stays on your machine.

1. **Start from the template** (section 1) and write your page (section 2).
2. **Build it:** `pnpm build` writes the unpacked app to `dist-app/`. To try the real package, also run
   `pnpm package -- --base-url https://example.org/unused/` (the url does not matter for a local install); it writes
   `out/` (section 4).
3. **Install from a folder.** Open the Armory, tab **Installed**, and choose **Install from folder…**. Pick one of:
   - your project folder: the Armory takes the `.bosapp` for this device from `out/` (or `dist/`), else `dist-app/`;
   - the `out/` or `dist-app/` folder itself, or any folder with an `app.json`;
   - or choose **Package file…** and pick the `.bosapp` file.

   The same checks as a catalog install run: size, the signature when a `.minisig` is beside the package (a package
   that does not match its signature is refused), the unzip limits, and `app.json` (id, version, contract, minOs). A
   package with no signature, or signed with a key only the folder vouches for (the template's throwaway key), asks
   first: **Unsigned local app: install it only if you built it.** Confirm, and the app installs and is marked
   **Local** in the Armory.
4. **Change it and install again.** Rebuild, then pick the folder again, or open the app's page in the Armory and
   choose **Reinstall from folder**. A newer `version` in `app.json` updates the app like a catalog update (stop,
   switch, start, rolled back if it never gets ready), and **Roll back** returns to the version before. The same
   version replaces the files in place.
5. **An app you installed from a catalog is never replaced silently.** Installing a local build of it asks
   **Replace the catalog app?** first. A local app is never updated from a catalog either: remove it, then install
   it from Browse.

### A local repo: test your catalog entry

To try the catalog path itself (list, install, update, roll back), use a local repo:

- **Publish into one.** An agent's app publish command (`--repo <name>`) writes a private repo under
  `<home>/local-repos/`, signed with this device's local key ([armory.md](armory.md)).
- **Or add a folder.** In the Armory's **Local repos** tab, type a name and choose **Add folder…**, then pick a folder
  holding `catalog.json`, for example a local checkout of `battalionOS-apps`. Channel urls in `catalog.json` and package
  urls in the feeds may be paths relative to the file they are in:

  ```
  catalog.json                  "channels": {"stable": "feeds/my-app/stable.json"}
  feeds/my-app/stable.json      "url": "../../packages/my-app-0.1.0-windows-x86_64.bosapp"
  packages/my-app-0.1.0-windows-x86_64.bosapp
  ```

  Its apps list in Browse next to the public ones, marked **Local** with the repo's name, and install, update and roll
  back through the same Armory path. Each entry keeps its own `publicKey`, pinned at install like any catalog's. An id
  the public catalog already lists stays the public one. Removing the repo removes only the Armory's link; your folder
  and its files stay.

## What is checked here

- `packages/app-api`: `pnpm test` (client over a fake bridge, typed errors, chunked reads, the local backend, types fresh
  from the schemas).
- `templates/page-app`: `pnpm typecheck`, `pnpm build`, `pnpm test`.
- `crates/battalion-host/tests/template_package.rs`: a package made by `package.mjs` (`fixtures/template-package`) installs
  through the OS's own Armory, and a changed one is refused.
- `crates/battalion-host/tests/local_folder.rs`: install from a folder (signed, unsigned with the confirm, tampered and
  bad-manifest refused, reinstall, rollback, a catalog app not replaced without asking) and a linked local repo
  (relative paths; list, install, update, rollback).
- `crates/battalion-host/tests/appstore.rs`: two apps never see each other's store.

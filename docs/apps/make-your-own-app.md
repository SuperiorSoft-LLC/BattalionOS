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

## What is checked here

- `packages/app-api`: `pnpm test` (client over a fake bridge, typed errors, chunked reads, the local backend, types fresh
  from the schemas).
- `templates/page-app`: `pnpm typecheck`, `pnpm build`, `pnpm test`.
- `crates/battalion-host/tests/template_package.rs`: a package made by `package.mjs` (`fixtures/template-package`) installs
  through the OS's own Armory, and a changed one is refused.
- `crates/battalion-host/tests/appstore.rs`: two apps never see each other's store.

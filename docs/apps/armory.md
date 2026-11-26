# The Armory: catalogs, install and update

The **Armory** is BattalionOS's app manager and its one updater: it lists apps from catalogs, installs them, updates
them (with rollback), and removes them, without restarting the OS. Truth: `crates/battalion-host/src/armory/`
(engine) and `crates/battalion-app-api/src/catalog.rs` (formats). The OS window's Armory screen and its commands
(`os_catalog`, `os_install_app`, `os_update_app`, `os_rollback_app`, `os_remove_app`) are in
[commands.md](../commands.md).

```text
catalog   sources (os.json) -> catalog v2 (lenient) -> offline cache <home>/catalog/
package   download -> size + sha256 -> minisign (pinned key) -> unzip (path, size, count) -> app.json
install   journaled: download, verify, unpack, place, init   (a crash is undone at the next start)
update    journaled: download, verify, unpack, place, stop, switch, start; a start that never gets ready is rolled back
remove    journaled: stop, mark, delete the versions, forget; the state folder stays unless asked
```

## Catalog sources

The catalogs the OS reads are urls (`https:` or `file:`) in `os.json` under `armory.sources`. On the first run, the
installer's `first-run.json` may seed them when the person has none ([packaging/README.md](../../packaging/README.md)).
When two sources list the same id, the first source wins.

A source that cannot be read is served from its last good copy in `<home>/catalog/`, and the listing says so
(`fromCache`). A source that answers with a broken file never replaces a good copy. Feeds are cached the same way.

## The catalog (schema 2)

```json
{
  "schema": 2,
  "apps": [
    {
      "id": "my-app",
      "name": "My App",
      "publisher": "My Co",
      "summary": "One line about the app.",
      "license": "Apache-2.0",
      "icon": "https://example.org/my-app.png",
      "homepage": "https://example.org",
      "publicKey": "RWQ...",
      "contract": { "min": 1, "max": 1 },
      "permissions": ["network"],
      "channels": { "stable": "https://example.org/my-app/stable.json", "beta": "https://example.org/my-app/beta.json" }
    }
  ]
}
```

| Field | Required | Check |
|---|---|---|
| `schema` | yes | Must be `2`; another value refuses the whole catalog. |
| `id` | yes | The app id rule (`a-z`, `0-9`, `-`, 2 to 63). Must equal the package's `app.json` `id`. Unique in the catalog. |
| `name` | yes | |
| `publisher`, `summary`, `license`, `homepage` | no | Shown in the Armory. |
| `icon` | no | Shown only when it is a `data:image/` or `https:` url. |
| `publicKey` | yes | The author's minisign public key ([package-and-sign.md](package-and-sign.md#the-author-key-and-the-signature)). Not empty. |
| `contract` | yes | `{min, max}`; an app whose range does not include the OS's contract is listed as not compatible. |
| `permissions` | no | What the app will ask for (its `app.json` `permissions` are what the OS enforces). |
| `channels` | yes | At least one: channel name to feed url (`https:` or `file:`). `stable` is the default channel. |
| `kind` | no | When set to anything but `app`, the entry is skipped and reported (the Armory lists apps only). |

The catalog is read leniently: a bad entry (bad id, no key, no channel, insecure url, duplicate, another kind) is
skipped and reported in the listing's `rejected` list; the other entries are still listed. The listing also shows each
app's `version` from its stable feed.

The feed each channel points at: [package-and-sign.md](package-and-sign.md#the-feed).

## Platforms

The OS takes the feed entry for its own platform key: `<os>-<arch>` on the desktop (`windows-x86_64`, `linux-x86_64`,
`macos-aarch64`), and always `android-aarch64` on Android, the x86_64 emulator included ([android.md](android.md)). A
feed with no entry for the platform cannot be installed there.

## Install

`os_install_app(id, channel?)` (default channel `stable`): the package check runs
([package-and-sign.md](package-and-sign.md#what-the-os-checks-in-order)), the version is placed under
`apps/<id>/`, `init` runs if the app has one, then `current.json` is written last, pinning the catalog key and
the channel. Progress is reported per step (`download`, `verify`, `unpack`, `place`, `init`). The install is all or
nothing: a crash mid-install is finished or undone by the journal at the next start.

One Armory action runs per app at a time (`apps/<id>/armory.lock`).

## Update and rollback

`os_update_app(id)` updates from the channel the app was installed on. The old version keeps running while the new one
downloads and is checked; then the app is stopped, `current.json` switches to the new version (keeping the old one as
`previous`), and the new version is started.

- **Staged, then the app gets ready** (UPDATE-LIVE-1): the new version is downloaded, verified and placed while the
  old one runs. Then a running app that names `prepare` in its ready file is asked to get ready
  ([create.md](create.md#prepare-for-an-update)) and asked again every `armory.preparePollMs` (1000) until it answers
  ready; an app without `prepare` is waited for until its health says `busy` 0. The update then applies by itself: no
  Retry. While it waits, the Armory shows the app's own words: "Infantry AI will update when its agents finish their
  current step (about 1 min)."
- **Not ready in time** (`armory.prepareWaitMs`, default 180000): the app is told to carry on and the staged version
  stays. The Armory offers **Update now** (the app checkpoints at once and is updated; the staged files are used) and
  **Later** (`os_update_later`: the version applies by itself at the app's next start, or the next moment it is idle,
  checked every `armory.pendingCheckMs`, default 60000). `armory.checkBusy: false` skips the wait (the app is stopped at once).
- **Refused** when the app is up to date, the signature is bad, or the package is signed with another key than the
  pinned one.
- **The app's view** closes and opens again on the new version by itself (`os://app-updated`).
- **Rolled back** when the new version never gets ready: with the same state schema, the old version is started
  again and the update reports it was rolled back.
- **Left failed** when the new version raised `state.schema` and did not start: it may already have changed the data,
  so the old version is not started on top of it. The old version stays on disk and a person decides.

`os_rollback_app(id)` goes back to the previous version by hand; refused when there is none or the state schema changed.
`armory.keepVersions` (default 2: current and previous) versions are kept on disk.

BattalionOS itself is not an app: it updates from its own signed feed ([commands.md](../commands.md#battalionos-updates-itself-bos-30));
on Android the OS updates through its store.

## Remove

`os_remove_app(id)` closes the app's window, stops it and deletes its versions. Its data folder (`data/<id>`) and web
view folder stay, so a later install finds its state again. Purging the default folders is a separate request; a state
folder the app named itself is never deleted by the OS.

## Settings

All under `armory` in `os.json`; a missing key takes the default.

| Setting | Default | Meaning |
|---|---|---|
| `sources` | `[]` | Catalog urls, first wins on a duplicate id. |
| `maxCatalogBytes` | 4 MiB | Largest catalog read. |
| `maxFeedBytes` | 256 KiB | Largest feed read. |
| `maxPackageBytes` | 2 GiB | Largest package; checked before it is stored. |
| `maxUnpackedBytes` | 8 GiB | Total bytes after unzipping. |
| `maxFileBytes` | 4 GiB | Largest single file in a package. |
| `maxEntries` | 100000 | Most entries in a package. |
| `keepVersions` | 2 | Versions kept per app. |
| `initTimeoutMs` | 120000 | How long `init` may run. |
| `checkBusy` | `true` | Ask a running app whether it is busy before an update. |
| `httpTimeoutMs` | 30000 | Network timeout. |
| `pagesOnly` | `true` on Android, else `false` | Refuse `init` and native content ([android.md](android.md)). |

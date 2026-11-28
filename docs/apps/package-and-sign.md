# Package and sign an app

To install through the Armory, an app is shipped as a signed **`.bosapp`** package, described by a **feed** and listed
in a **catalog** ([armory.md](armory.md)). Truth: `crates/battalion-app-api/src/catalog.rs` (formats) and
`crates/battalion-host/src/armory/package.rs` (the check).

## The package: .bosapp

A `.bosapp` is a zip file. At its root is `app.json` ([create.md](create.md)); everything the manifest names (library,
page folder, icon, `init` and `cli` programs) is inside it at those paths. One package per platform: a Windows
package holds the `.dll`, an Android package holds only the page.

The check unzips it with limits (each is a setting under `armory` in `os.json`, see [armory.md](armory.md#settings)):
no entry may leave the folder, at most `maxEntries` entries (100000), `maxFileBytes` per file (4 GiB) and
`maxUnpackedBytes` in all (8 GiB); the download itself is capped at `maxPackageBytes` (2 GiB) and at the size the feed
gives.

## The author key and the signature

An app's author signs every package with one **minisign** key:

- The **public key** goes in the catalog entry as `publicKey`: the base64 line of the minisign `.pub` file (a whole
  `.pub` file is accepted too).
- The **signature** goes in the feed as `signature`: the text of the package's `.minisig` file. It covers the package
  bytes. Only prehashed signatures (`ED`, what `minisign -S` writes today) are accepted; the legacy `Ed` mode is
  refused.

```
minisign -G -p my-app.pub -s my-app.key          # once: make the author key
minisign -S -s my-app.key -m my-app-1.2.0-windows-x86_64.bosapp   # writes the .minisig beside it
```

**The key is pinned.** The first install stores the catalog's key in the app's `current.json`. Every later update is
checked against that pinned key, never against a newer key in the catalog, so a changed catalog cannot hand an
installed app to someone else. An update signed with another key is refused ("other key"). Keep the secret key safe:
losing it means existing installs cannot be updated, only removed and installed again.

## The feed

One feed per channel of an app (a channel url in the catalog points at it). It says what the newest version is and
where each platform's package is:

```json
{
  "version": "1.2.0",
  "notes": "What changed.",
  "contract": 1,
  "stateSchema": 1,
  "platforms": {
    "windows-x86_64":  { "url": "https://example.org/my-app-1.2.0-windows-x86_64.bosapp",  "signature": "untrusted comment: ...\nRUQ...", "sha256": "<64 hex>", "size": 1234567 },
    "android-aarch64": { "url": "https://example.org/my-app-1.2.0-android-aarch64.bosapp", "signature": "...", "sha256": "<64 hex>", "size": 45678 }
  }
}
```

| Field | Check |
|---|---|
| `version` | `major.minor.patch`; the package's `app.json` must have the same version. |
| `notes` | Optional text shown with an update. |
| `contract` | Must not be newer than the OS's contract. |
| `stateSchema` | Default `1`; tells readers the version's data layout. The OS itself goes by the installed package's `state.schema`. |
| `platforms` | At least one. Keys are platform keys (`windows-x86_64`, `linux-x86_64`, `macos-aarch64`, `android-aarch64`). Each has `url` (`https://` or `file:`), `signature` (not empty), `sha256` (64 hex characters) and `size` (bytes). |

The OS takes the entry for its own platform; with none, the install is refused for that platform.

## What the OS checks, in order

Nothing real is touched until every step passes (everything is written under `<home>/staging/` first):

1. **Plan:** the app's `contract` range and the feed's `contract` against the OS; the feed has this platform.
2. **Download** with the size cap; the size must equal `size`, then **sha256** must match.
3. **Signature:** minisign over the package bytes, with the pinned key (the catalog's key on first install).
4. **Unpack** with the limits above.
5. **Manifest:** a root `module.json` without `app.json` is refused ("this package is not an app"); `app.json` must be
   valid, its `id` must equal the catalog id, its `version` the feed version, `minOs` must not be newer than the OS, and
   the process library must be in the package.
6. **Pages only** (Android): no `init` and no native code ([android.md](android.md)).

## Making a package

For a page app, the template's packer does all of it in Node: `templates/page-app/scripts/package.mjs` (APP-API-1; see
[Make your own app](make-your-own-app.md#4-package-and-sign)). It takes the minisign key from the environment only. Any other tool
that produces the same files works too: an author zips the app folder, signs the zip with minisign, and writes the feed and a
catalog entry.

Example from an app: the Infantry AI repository's `scripts/bos-package.py` (branch `battalion-os`) packs Infantry AI.
It writes `infantry-ai-<version>-<platform>.bosapp`, its `.bosapp.sig`, `feed.json`, `catalog-entry.json` and
`public.key`. Without `--minisign-key` (or `--tauri-sign` in CI) it makes a **throwaway** key: fine for a local test,
never for a real release. `--base-url` sets where the package will be served (default: a `file:` url of the output).

For a local test, a `file:` catalog and feed work: `packaging/make-sandbox-kit.ps1` builds a Windows Sandbox kit from
such a package (see [packaging/README.md](../../packaging/README.md)).

Next: [list it in a catalog](armory.md).

#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// Pack and sign the built app (`pnpm build` first) for the Armory (docs/apps/package-and-sign.md):
//   out/<id>-<version>-<platform>.bosapp  (+ .minisig)   one per platform; a page-only app is the same content on each
//   out/feed.json                                         version, contract, and per platform: url, signature, sha256, size
//   out/catalog-entry.json                                the app's entry for a catalog (publicKey, channels, platforms)
//   out/public.key                                        the base64 line of the signing key's .pub
//
// The signing key comes from the ENVIRONMENT only, never from a file this script opens:
//   BOS_APP_SIGNING_KEY            the text of a minisign secret key (the .key file's content)
//   BOS_APP_SIGNING_KEY_PASSWORD   its password, when it has one
// Without BOS_APP_SIGNING_KEY it signs with a THROWAWAY key (printed as such): fine for a local test, never for a release.
//
// Options: --app <folder> (default dist-app)  --out <folder> (default out)  --base-url <https://.../> (default: file: urls
// of the out folder)  --platforms windows-x86_64,android-aarch64  --channel stable
import { createHash } from "node:crypto";
import { mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";
import { parseSecretKey, publicKeyLine, signBytes, throwawayKey } from "./minisign.mjs";
import { zip } from "./zip.mjs";

function args(argv) {
  const o = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (!a.startsWith("--")) throw new Error(`unexpected '${a}'`);
    o[a.slice(2)] = argv[++i];
  }
  return o;
}

function files(dir) {
  const out = [];
  const walk = (d) => {
    for (const name of readdirSync(d).sort()) {
      const p = join(d, name);
      if (statSync(p).isDirectory()) walk(p);
      else out.push({ name: relative(dir, p).split(sep).join("/"), data: readFileSync(p) });
    }
  };
  walk(dir);
  return out;
}

/**
 * @typedef {{ url: string, signature: string, sha256: string, size: number }} FeedPlatform
 * @param {{ app?: string, out?: string, baseUrl?: string, platforms?: string[], channel?: string,
 *   env?: Record<string, string | undefined>, now?: number, log?: (s: string) => void }} [opts]
 */
export function pack({ app = "dist-app", out = "out", baseUrl, platforms = ["windows-x86_64", "android-aarch64"], channel = "stable", env = process.env, now = Date.now(), log = console.log } = {}) {
  const appDir = resolve(app);
  const manifest = JSON.parse(readFileSync(join(appDir, "app.json"), "utf8"));
  for (const f of ["id", "name", "version"]) if (typeof manifest[f] !== "string") throw new Error(`app.json has no ${f}`);
  if (!manifest.ui?.entry) throw new Error("app.json has no ui.entry (a page app)");
  const entries = files(appDir);
  if (!entries.some((e) => e.name === manifest.ui.entry)) throw new Error(`${manifest.ui.entry} is not in ${app}: run pnpm build first`);

  let key;
  let trusted = true;
  if (env.BOS_APP_SIGNING_KEY) {
    key = parseSecretKey(env.BOS_APP_SIGNING_KEY, env.BOS_APP_SIGNING_KEY_PASSWORD ?? "");
  } else {
    key = throwawayKey();
    trusted = false;
    log("No BOS_APP_SIGNING_KEY: signed with a THROWAWAY key. Fine for a local test; never publish this package.");
  }

  const outDir = resolve(out);
  mkdirSync(outDir, { recursive: true });
  const bytes = zip(entries, now);
  const base = baseUrl ? (baseUrl.endsWith("/") ? baseUrl : baseUrl + "/") : pathToFileURL(outDir + sep).href;
  /** @type {{ version: string, contract: number, stateSchema: number, platforms: Record<string, FeedPlatform> }} */
  const feed = { version: manifest.version, contract: manifest.contract?.max ?? 1, stateSchema: manifest.state?.schema ?? 1, platforms: {} };
  for (const platform of platforms) {
    const name = `${manifest.id}-${manifest.version}-${platform}.bosapp`;
    const signature = signBytes(key, bytes, name, now);
    writeFileSync(join(outDir, name), bytes);
    writeFileSync(join(outDir, name + ".minisig"), signature);
    feed.platforms[platform] = { url: base + name, signature, sha256: createHash("sha256").update(bytes).digest("hex"), size: bytes.length };
  }
  const pub = publicKeyLine(key);
  writeFileSync(join(outDir, "feed.json"), JSON.stringify(feed, null, 2) + "\n");
  const entry = {
    id: manifest.id,
    name: manifest.name,
    publisher: manifest.publisher,
    publicKey: pub,
    contract: manifest.contract ?? { min: 1, max: 1 },
    channels: { [channel]: base + "feed.json" },
    platforms,
  };
  writeFileSync(join(outDir, "catalog-entry.json"), JSON.stringify(entry, null, 2) + "\n");
  writeFileSync(join(outDir, "public.key"), pub + "\n");
  log(`Packed ${manifest.id} ${manifest.version} for ${platforms.join(", ")} into ${outDir}${trusted ? "" : " (NOT TRUSTED: throwaway key)"}`);
  return { outDir, feed, entry, trusted };
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    const o = args(process.argv.slice(2));
    pack({ app: o.app, out: o.out, baseUrl: o["base-url"], platforms: o.platforms?.split(","), channel: o.channel });
  } catch (e) {
    console.error(`package: ${e.message}`);
    process.exit(1);
  }
}

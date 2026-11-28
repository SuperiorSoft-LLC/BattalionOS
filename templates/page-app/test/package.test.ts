// SPDX-License-Identifier: Apache-2.0
// The pack-and-sign script: a throwaway-signed package verifies; a key from the environment (with and without a password)
// signs; a wrong password is refused. No real key is ever read: every key here is made in the test.
import { createHash, createPublicKey, randomBytes, scryptSync, verify } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { parseSecretKey, throwawayKey } from "../scripts/minisign.mjs";
import { pack } from "../scripts/package.mjs";

const SPKI_ED25519 = Buffer.from("302a300506032b6570032100", "hex");

/** Check a .minisig like minisign-verify does: ED prehashed signature, then the global signature over sig + trusted comment. */
function check(sigText: string, data: Buffer, pubLine: string) {
  const pub = Buffer.from(pubLine, "base64");
  const key = createPublicKey({ key: Buffer.concat([SPKI_ED25519, pub.subarray(10)]), format: "der", type: "spki" });
  const [, sigLine, trustedLine, globalLine] = sigText.split("\n");
  const sig = Buffer.from(sigLine!, "base64");
  expect(sig.subarray(0, 2).toString()).toBe("ED");
  expect(sig.subarray(2, 10)).toEqual(pub.subarray(2, 10));
  expect(verify(null, createHash("blake2b512").update(data).digest(), key, sig.subarray(10))).toBe(true);
  const trusted = trustedLine!.replace("trusted comment: ", "");
  expect(verify(null, Buffer.concat([sig.subarray(10), Buffer.from(trusted)]), key, Buffer.from(globalLine!, "base64"))).toBe(true);
}

/** A minisign .key text for `k`, unprotected or with a password (small scrypt limits so the test is fast). */
function keyText(k: ReturnType<typeof throwawayKey>, password?: string): string {
  const salt = randomBytes(32);
  const ops = 32768n;
  const mem = 16n * 1024n * 1024n;
  const body = Buffer.concat([k.keyId, k.seed, k.publicKey, Buffer.alloc(32)]);
  let enc = body;
  if (password) {
    // ops < mem/32: p = 1, N from ops (libsodium's pickparams).
    const maxN = ops / 32n;
    let n = 1n;
    while (n < 63n && !(1n << n > maxN / 2n)) n++;
    const stream = scryptSync(password, salt, 104, { N: Number(1n << n), r: 8, p: 1 });
    enc = Buffer.from(body.map((x, i) => x ^ stream[i]!));
  }
  const head = Buffer.alloc(54);
  head.write("Ed", 0);
  head.write(password ? "Sc" : "\0\0", 2, "latin1");
  head.write("B2", 4);
  salt.copy(head, 6);
  head.writeBigUInt64LE(ops, 38);
  head.writeBigUInt64LE(mem, 46);
  return `untrusted comment: minisign encrypted secret key\n${Buffer.concat([head, enc]).toString("base64")}\n`;
}

function appDir(): string {
  const d = mkdtempSync(join(tmpdir(), "bos-pack-"));
  mkdirSync(join(d, "app", "ui"), { recursive: true });
  writeFileSync(join(d, "app", "app.json"), readFileSync("app.json"));
  writeFileSync(join(d, "app", "ui", "index.html"), "<h1>hi</h1>");
  return d;
}

describe("package.mjs", () => {
  it("packs and signs with a throwaway key when no key is in the environment", () => {
    const d = appDir();
    const logs: string[] = [];
    const r = pack({ app: join(d, "app"), out: join(d, "out"), env: {}, log: (s: string) => logs.push(s) });
    expect(r.trusted).toBe(false);
    expect(logs.join("\n")).toMatch(/THROWAWAY/);
    const pub = readFileSync(join(d, "out", "public.key"), "utf8").trim();
    for (const [platform, p] of Object.entries(r.feed.platforms) as [string, { url: string; signature: string; sha256: string; size: number }][]) {
      const bytes = readFileSync(join(d, "out", `my-app-0.1.0-${platform}.bosapp`));
      expect(p.size).toBe(bytes.length);
      expect(p.sha256).toBe(createHash("sha256").update(bytes).digest("hex"));
      expect(p.url.startsWith("file:")).toBe(true);
      check(p.signature, bytes, pub);
    }
    expect(r.entry).toMatchObject({ id: "my-app", publicKey: pub, platforms: ["windows-x86_64", "android-aarch64"] });
  });
  it("signs with the key from the environment, with or without a password", () => {
    for (const password of [undefined, "correct horse"]) {
      const d = appDir();
      const k = throwawayKey();
      const env = { BOS_APP_SIGNING_KEY: keyText(k, password), ...(password ? { BOS_APP_SIGNING_KEY_PASSWORD: password } : {}) };
      const r = pack({ app: join(d, "app"), out: join(d, "out"), env, baseUrl: "https://example.org/rel", log: () => undefined });
      expect(r.trusted).toBe(true);
      const pubLine = Buffer.concat([Buffer.from("Ed"), k.keyId, k.publicKey]).toString("base64");
      expect(r.entry.publicKey).toBe(pubLine);
      expect(r.feed.platforms["windows-x86_64"].url).toBe("https://example.org/rel/my-app-0.1.0-windows-x86_64.bosapp");
      check(r.feed.platforms["windows-x86_64"].signature, readFileSync(join(d, "out", "my-app-0.1.0-windows-x86_64.bosapp")), pubLine);
    }
  });
  it("refuses a wrong or missing password, and a missing build", () => {
    const k = throwawayKey();
    expect(() => parseSecretKey(keyText(k, "right"), "wrong")).toThrow(/does not open/);
    expect(() => parseSecretKey(keyText(k, "right"))).toThrow(/password/);
    const d = mkdtempSync(join(tmpdir(), "bos-pack-"));
    mkdirSync(join(d, "app"));
    writeFileSync(join(d, "app", "app.json"), readFileSync("app.json"));
    expect(() => pack({ app: join(d, "app"), out: join(d, "out"), env: {}, log: () => undefined })).toThrow(/pnpm build/);
  });
});

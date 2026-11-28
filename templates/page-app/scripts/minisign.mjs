// SPDX-License-Identifier: Apache-2.0
// Minisign in plain Node (no minisign binary, no Python): read a secret key from TEXT (never a file path here), sign a file
// the way `minisign -S` does (prehashed "ED": Ed25519 over BLAKE2b-512 of the bytes), make a throwaway key for a local test.
// The OS checks the result with minisign-verify (docs/apps/package-and-sign.md).
import { createHash, createPrivateKey, createPublicKey, randomBytes, scryptSync, sign } from "node:crypto";

const PKCS8_ED25519 = Buffer.from("302e020100300506032b657004220420", "hex");

function keyFromSeed(seed) {
  return createPrivateKey({ key: Buffer.concat([PKCS8_ED25519, seed]), format: "der", type: "pkcs8" });
}
function publicOf(seed) {
  const der = createPublicKey(keyFromSeed(seed)).export({ format: "der", type: "spki" });
  return der.subarray(der.length - 32);
}

/** libsodium's crypto_pwhash_scryptsalsa208sha256 parameters for (opslimit, memlimit), as minisign uses them. */
function scryptParams(opslimit, memlimit) {
  let ops = opslimit < 32768n ? 32768n : opslimit;
  const r = 8n;
  let nLog2 = 1n;
  let p;
  if (ops < memlimit / 32n) {
    p = 1n;
    const maxN = ops / (r * 4n);
    for (nLog2 = 1n; nLog2 < 63n; nLog2++) if (1n << nLog2 > maxN / 2n) break;
  } else {
    const maxN = memlimit / (r * 128n);
    for (nLog2 = 1n; nLog2 < 63n; nLog2++) if (1n << nLog2 > maxN / 2n) break;
    let maxrp = ops / 4n / (1n << nLog2);
    if (maxrp > 0x3fffffffn) maxrp = 0x3fffffffn;
    p = maxrp / r;
  }
  return { N: Number(1n << nLog2), r: Number(r), p: Number(p) };
}

/**
 * The secret key of a minisign `.key` file's TEXT (both lines, or only the base64 line), decrypted with `password` when the
 * key has one (`minisign -G`; `minisign -G -W` makes one without). Refuses a wrong password (the key does not match itself).
 */
export function parseSecretKey(text, password = "") {
  const line = text.split(/\r?\n/).map((l) => l.trim()).filter((l) => l && !l.startsWith("untrusted comment:"))[0];
  if (!line) throw new Error("no minisign secret key in the text");
  const b = Buffer.from(line, "base64");
  if (b.length !== 158 || b.subarray(0, 2).toString() !== "Ed") throw new Error("not a minisign Ed25519 secret key");
  const kdf = b.subarray(2, 4).toString("latin1");
  const salt = b.subarray(6, 38);
  const ops = b.readBigUInt64LE(38);
  const mem = b.readBigUInt64LE(46);
  let enc = Buffer.from(b.subarray(54, 158)); // keynum(8) sk(64) checksum(32)
  if (kdf === "Sc") {
    if (!password) throw new Error("this key has a password: set BOS_APP_SIGNING_KEY_PASSWORD");
    const { N, r, p } = scryptParams(ops, mem);
    const stream = scryptSync(Buffer.from(password, "utf8"), salt, 104, { N, r, p, maxmem: 128 * N * r * p + 64 * 1024 * 1024 });
    enc = Buffer.from(enc.map((x, i) => x ^ stream[i]));
  } else if (kdf !== "\0\0") {
    throw new Error(`unknown key protection '${kdf}'`);
  }
  const keyId = enc.subarray(0, 8);
  const sk = enc.subarray(8, 72);
  const seed = sk.subarray(0, 32);
  if (!publicOf(seed).equals(sk.subarray(32, 64))) throw new Error("the key does not open: wrong BOS_APP_SIGNING_KEY_PASSWORD?");
  return { keyId: Buffer.from(keyId), seed: Buffer.from(seed), publicKey: Buffer.from(sk.subarray(32, 64)) };
}

/** A new key held in memory only (for a local test package; never for a release). */
export function throwawayKey() {
  const seed = randomBytes(32);
  return { keyId: randomBytes(8), seed, publicKey: publicOf(seed) };
}

/** The base64 line of the key's `.pub` file (what a catalog entry's `publicKey` holds). */
export function publicKeyLine(key) {
  return Buffer.concat([Buffer.from("Ed"), key.keyId, key.publicKey]).toString("base64");
}

/** The text of the `.minisig` file for `data` (prehashed, what `minisign -S` writes). */
export function signBytes(key, data, fileName, now = Date.now()) {
  const pk = keyFromSeed(key.seed);
  const sig = sign(null, createHash("blake2b512").update(data).digest(), pk);
  const trusted = `timestamp:${Math.floor(now / 1000)}\tfile:${fileName}\thashed`;
  const global = sign(null, Buffer.concat([sig, Buffer.from(trusted)]), pk);
  return [
    "untrusted comment: signature from minisign secret key",
    Buffer.concat([Buffer.from("ED"), key.keyId, sig]).toString("base64"),
    `trusted comment: ${trusted}`,
    global.toString("base64"),
    "",
  ].join("\n");
}

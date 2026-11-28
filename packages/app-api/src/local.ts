// SPDX-License-Identifier: Apache-2.0
// Copyright (c) SuperiorSoft LLC
// APP-API-1: the local backend. Runs the page in a plain browser (or a test) with no OS: an in-memory file tree under one
// root and an in-memory store (optionally kept in a `Storage` such as localStorage). It answers the same calls with the
// same result shapes and the same `... [code]` refusals as the OS, so code written against it runs under BattalionOS as is.
// It is a development aid, not the OS's rules: limits are small fixed defaults and there are no links.
import type { Transport } from "./transport";

type Node = { kind: "dir"; children: Map<string, Node>; modified: number } | { kind: "file"; data: Uint8Array; modified: number };

export interface LocalOptions {
  /** Files to start with: path -> text. Folders are made as needed. */
  files?: Record<string, string>;
  /** Where the store is kept between reloads (e.g. `localStorage`); in memory without one. */
  storage?: Pick<Storage, "getItem" | "setItem">;
  /** The storage key; one per app when several share an origin. */
  storageKey?: string;
  /** Same meaning as the OS settings, smaller defaults. */
  readMaxBytes?: number;
  listMax?: number;
  storeMaxBytes?: number;
  /** Called for `os_notify` (default: nothing). */
  onNotify?: (title: string, text?: string) => void;
}

class Refusal extends Error {
  constructor(message: string, code: string) {
    super(`${message} [${code}]`);
  }
}

const enc = new TextEncoder();
const dec = new TextDecoder("utf-8", { fatal: true });

function b64(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}
function unb64(s: string): Uint8Array {
  const t = atob(s);
  const out = new Uint8Array(t.length);
  for (let i = 0; i < t.length; i++) out[i] = t.charCodeAt(i);
  return out;
}

/** A relative path as segments; `..`, absolute paths and empty segments are refused like the OS refuses them. */
function segs(path: unknown): string[] {
  if (path === undefined || path === null || path === "") return [];
  if (typeof path !== "string") throw new Refusal("path is a string", "bad_args");
  if (/^([a-zA-Z]:|[\\/])/.test(path)) throw new Refusal("path refused: an absolute path", "leaves_root");
  const out = path.split(/[\\/]/).filter((s) => s !== "" && s !== ".");
  if (out.includes("..")) throw new Refusal("path refused: '..'", "leaves_root");
  return out;
}

export function localTransport(opts: LocalOptions = {}): Transport {
  const readMax = opts.readMaxBytes ?? 1024 * 1024;
  const listMax = opts.listMax ?? 1000;
  const storeMax = opts.storeMaxBytes ?? 1024 * 1024;
  const now = () => Date.now();
  const root: Node = { kind: "dir", children: new Map(), modified: now() };
  const listeners = new Set<(m: Record<string, unknown>) => void>();
  const watches = new Map<string, string[]>();
  let watchN = 0;

  const find = (p: string[]): Node | undefined => {
    let n: Node = root;
    for (const s of p) {
      if (n.kind !== "dir") return undefined;
      const next = n.children.get(s);
      if (!next) return undefined;
      n = next;
    }
    return n;
  };
  const must = (p: string[]): Node => {
    const n = find(p);
    if (!n) throw new Refusal("no such file or folder", "not_found");
    return n;
  };
  const parentOf = (p: string[]): { dir: Extract<Node, { kind: "dir" }>; name: string } => {
    if (p.length === 0) throw new Refusal("the root itself cannot be changed", "is_root");
    const dir = must(p.slice(0, -1));
    if (dir.kind !== "dir") throw new Refusal("not a folder", "not_a_folder");
    return { dir, name: p[p.length - 1]! };
  };
  const changed = (p: string[], change: "created" | "changed" | "removed") => {
    const path = p.join("/");
    for (const [watch, w] of watches) {
      const wp = w.join("/");
      const parent = p.slice(0, -1).join("/");
      if (wp === path || wp === parent) for (const l of listeners) l({ bos: 1, event: "os_fs_changed", watch, path, change });
    }
  };
  const put = (p: string[], data: Uint8Array) => {
    const { dir, name } = parentOf(p);
    const had = dir.children.get(name);
    if (had?.kind === "dir") throw new Refusal("a folder has that name", "exists");
    dir.children.set(name, { kind: "file", data, modified: now() });
    dir.modified = now();
    changed(p, had ? "changed" : "created");
  };
  for (const [path, text] of Object.entries(opts.files ?? {})) {
    const p = segs(path);
    for (let i = 1; i < p.length; i++) {
      const d = find(p.slice(0, i));
      if (!d) parentOf(p.slice(0, i)).dir.children.set(p[i - 1]!, { kind: "dir", children: new Map(), modified: now() });
    }
    put(p, enc.encode(text));
  }
  const clone = (n: Node): Node => (n.kind === "file" ? { ...n, data: n.data.slice() } : { kind: "dir", modified: now(), children: new Map([...n.children].map(([k, v]) => [k, clone(v)])) });
  const count = (n: Node): number => (n.kind === "file" ? 1 : 1 + [...n.children.values()].reduce((a, c) => a + count(c), 0));

  // The store.
  const key = opts.storageKey ?? "bos-app-store";
  let values: Record<string, unknown> = {};
  try {
    values = JSON.parse(opts.storage?.getItem(key) ?? "{}") as Record<string, unknown>;
  } catch {
    values = {};
  }
  const storeKey = (args: Record<string, unknown>): string => {
    const k = args.key;
    if (typeof k !== "string" || k.length === 0 || k.length > 256 || /[\u0000-\u001f]/.test(k)) throw new Refusal("a key is 1 to 256 characters, no control characters", "bad_args");
    return k;
  };

  const handle = (cmd: string, a: Record<string, unknown>): unknown => {
    switch (cmd) {
      case "os_fs_list": {
        const n = must(segs(a.path));
        if (n.kind !== "dir") throw new Refusal("not a folder", "not_a_folder");
        const all = [...n.children.entries()].sort(([x], [y]) => (x < y ? -1 : x > y ? 1 : 0));
        const start = typeof a.cursor === "string" ? Number(a.cursor) || 0 : 0;
        const limit = Math.max(1, Math.min(typeof a.limit === "number" ? a.limit : listMax, listMax));
        const page = all.slice(start, start + limit);
        const next = start + page.length;
        return {
          entries: page.map(([name, c]) => ({ name, kind: c.kind, size: c.kind === "file" ? c.data.length : 0, modified: c.modified })),
          cursor: next < all.length ? String(next) : null,
        };
      }
      case "os_fs_stat": {
        const p = segs(a.path);
        const n = must(p);
        return { name: p[p.length - 1] ?? "", kind: n.kind, target: n.kind, size: n.kind === "file" ? n.data.length : 0, modified: n.modified, created: n.modified, readonly: false, display: "/" + p.join("/") };
      }
      case "os_fs_read": {
        const n = must(segs(a.path));
        if (n.kind !== "file") throw new Refusal("not a file", "not_a_file");
        const size = n.data.length;
        const offset = Math.min(typeof a.offset === "number" ? a.offset : 0, size);
        let end = Math.min(offset + Math.min(typeof a.length === "number" ? a.length : readMax, readMax), size);
        if (a.as === "base64") return { data: b64(n.data.subarray(offset, end)), size, offset, length: end - offset, eof: end >= size };
        // Text: end on a character boundary (never inside a UTF-8 sequence).
        while (end < size && end > offset && (n.data[end]! & 0xc0) === 0x80) end--;
        let data: string;
        try {
          data = dec.decode(n.data.subarray(offset, end));
        } catch {
          throw new Refusal("not UTF-8 text; read it as base64", "not_text");
        }
        return { data, size, offset, length: end - offset, eof: end >= size };
      }
      case "os_fs_write": {
        const p = segs(a.path);
        if (typeof a.data !== "string") throw new Refusal("os_fs_write needs data", "bad_args");
        const data = a.as === "base64" ? unb64(a.data) : enc.encode(a.data);
        if (a.overwrite === false && find(p)) throw new Refusal("that name is taken", "exists");
        put(p, data);
        return { size: data.length };
      }
      case "os_fs_mkdir": {
        const p = segs(a.path);
        const { dir, name } = parentOf(p);
        if (dir.children.has(name)) throw new Refusal("that name is taken", "exists");
        dir.children.set(name, { kind: "dir", children: new Map(), modified: now() });
        changed(p, "created");
        return null;
      }
      case "os_fs_rename":
      case "os_fs_copy": {
        const from = segs(a.from);
        const to = segs(a.to);
        const src = must(from);
        const { dir: fromDir, name: fromName } = parentOf(from);
        const { dir, name } = parentOf(to);
        if (dir.children.has(name)) throw new Refusal("that name is taken", "exists");
        if (to.join("/").startsWith(from.join("/") + "/")) throw new Refusal("a folder cannot go into itself", "bad_args");
        if (cmd === "os_fs_copy") {
          dir.children.set(name, clone(src));
          changed(to, "created");
          return { entries: count(src) };
        }
        fromDir.children.delete(fromName);
        dir.children.set(name, src);
        changed(from, "removed");
        changed(to, "created");
        return null;
      }
      case "os_fs_trash":
      case "os_fs_delete": {
        const p = segs(a.path);
        const n = must(p);
        const { dir, name } = parentOf(p);
        if (cmd === "os_fs_delete" && n.kind === "dir" && n.children.size > 0 && a.recursive !== true) throw new Refusal("the folder is not empty", "not_empty");
        dir.children.delete(name);
        changed(p, "removed");
        return null;
      }
      case "os_fs_watch": {
        const p = segs(a.path);
        must(p);
        const watch = `w${++watchN}`;
        watches.set(watch, p);
        return { watch };
      }
      case "os_fs_unwatch":
        if (typeof a.watch !== "string" || !watches.delete(a.watch)) throw new Refusal("no such watch", "no_such_watch");
        return null;
      case "os_store_get":
        return values[storeKey(a)] ?? null;
      case "os_store_set": {
        const k = storeKey(a);
        const next = { ...values };
        if (a.value === null || a.value === undefined) delete next[k];
        else next[k] = a.value;
        const text = JSON.stringify(next);
        if (enc.encode(text).length > storeMax) throw new Refusal(`the store would be more than ${storeMax} bytes (store.maxBytes)`, "too_large");
        values = next;
        opts.storage?.setItem(key, text);
        return null;
      }
      case "os_store_delete": {
        const k = storeKey(a);
        const had = k in values;
        delete values[k];
        opts.storage?.setItem(key, JSON.stringify(values));
        return had;
      }
      case "os_store_list": {
        if (a.prefix !== undefined && typeof a.prefix !== "string") throw new Refusal("prefix is a string", "bad_args");
        const pre = (a.prefix as string | undefined) ?? "";
        return Object.keys(values).filter((k) => k.startsWith(pre)).sort();
      }
      case "os_notify":
        opts.onNotify?.(String(a.title ?? ""), typeof a.text === "string" ? a.text : undefined);
        return null;
      case "os_badge":
        return null;
      default:
        throw new Error(`'${cmd}' is not available in the local backend (run the app under BattalionOS)`);
    }
  };

  return {
    async send(cmd, args) {
      return handle(cmd, args ?? {});
    },
    listen(fn) {
      listeners.add(fn);
      return () => void listeners.delete(fn);
    },
  };
}

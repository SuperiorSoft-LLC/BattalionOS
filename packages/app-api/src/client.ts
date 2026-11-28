// SPDX-License-Identifier: Apache-2.0
// Copyright (c) SuperiorSoft LLC
// APP-API-1: the promise client. Every method is one documented OS call (docs/commands.md); errors are typed (errors.ts).
import { toError } from "./errors";
import type { FsChangedEvent, FsCopy, FsEncoding, FsEntry, FsList, FsRead, FsStat, FsWatch, FsWrite } from "./fs.gen";
import type { Transport } from "./transport";
import type { Opened, SnapZone, WinEvent, WindowInfo, WindowPatch, WindowSpec } from "./window.gen";

export interface Bos {
  /** `os`: under BattalionOS. `local`: the local backend (a plain browser, tests). */
  readonly backend: "os" | "local";
  /** Any call by name (a newer OS call this client does not wrap yet). */
  call<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T>;
  /** Every OS event for this page. Returns the unsubscribe. */
  onEvent(fn: (event: Record<string, unknown>) => void): () => void;
  fs: FsApi;
  store: StoreApi;
  windows: WindowsApi;
  /** A notice on the app's own channel (`notifications`). */
  notify(title: string, text?: string, opts?: { tag?: string; route?: string }): Promise<void>;
  /** The app's badge (a count, a short text, or neither to clear it). */
  badge(count?: number, text?: string): Promise<void>;
  /** The person chooses a folder / a file (`files.user`): the path, or null when cancelled. */
  pickFolder(title?: string): Promise<string | null>;
  pickFile(title?: string): Promise<string | null>;
  /** A link in the system browser (`network`): http, https, mailto. */
  openExternal(url: string): Promise<void>;
}

export interface FsApi {
  list(path?: string, opts?: { cursor?: string; limit?: number }): Promise<FsList>;
  /** Every entry of a folder, following the pages (`cursor`). */
  listAll(path?: string): Promise<FsEntry[]>;
  stat(path?: string): Promise<FsStat>;
  read(path: string, opts?: { offset?: number; length?: number; as?: FsEncoding }): Promise<FsRead>;
  /** The whole text file, read in chunks of at most `fs.readMaxBytes`. */
  readText(path: string): Promise<string>;
  /** The whole file as bytes, read in base64 chunks. */
  readBytes(path: string): Promise<Uint8Array>;
  write(path: string, data: string, opts?: { as?: FsEncoding; overwrite?: boolean }): Promise<FsWrite>;
  writeText(path: string, text: string, opts?: { overwrite?: boolean }): Promise<FsWrite>;
  writeBytes(path: string, bytes: Uint8Array, opts?: { overwrite?: boolean }): Promise<FsWrite>;
  mkdir(path: string): Promise<void>;
  rename(from: string, to: string): Promise<void>;
  copy(from: string, to: string): Promise<FsCopy>;
  trash(path: string): Promise<void>;
  delete(path: string, opts?: { recursive?: boolean }): Promise<void>;
  /** Watch a file or folder; `fn` gets its changes. Resolves with the stop function. */
  watch(path: string, fn: (e: FsChangedEvent) => void): Promise<() => Promise<void>>;
}

export interface StoreApi {
  /** The value, or `undefined` when the key is not set. */
  get<T = unknown>(key: string): Promise<T | undefined>;
  /** Any JSON value; `null`/`undefined` deletes the key. The whole store is at most `store.maxBytes` (1 MiB by default). */
  set(key: string, value: unknown): Promise<void>;
  /** True when the key was there. */
  delete(key: string): Promise<boolean>;
  /** The keys, sorted, that start with `prefix`. */
  list(prefix?: string): Promise<string[]>;
}

export interface WindowsApi {
  open(spec: WindowSpec): Promise<Opened>;
  close(wid: string): Promise<void>;
  minimize(wid: string): Promise<void>;
  restore(wid: string): Promise<void>;
  maximize(wid: string): Promise<void>;
  focus(wid: string): Promise<void>;
  snap(wid: string, zone: SnapZone): Promise<void>;
  set(wid: string, patch: WindowPatch): Promise<void>;
  list(): Promise<WindowInfo[]>;
  onEvent(fn: (e: WinEvent) => void): () => void;
}

/** base64 <-> bytes without Node's Buffer (pages, workers and Node 18+ all have atob/btoa). */
export function toBase64(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}
export function fromBase64(b64: string): Uint8Array {
  const s = atob(b64);
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out;
}

function opt(o: Record<string, unknown>): Record<string, unknown> {
  return Object.fromEntries(Object.entries(o).filter(([, v]) => v !== undefined));
}

/** A client over `transport`. `connect()` (index.ts) picks the transport for you. */
export function createBos(transport: Transport, backend: "os" | "local" = "os"): Bos {
  const call = async <T = unknown>(cmd: string, args: Record<string, unknown> = {}): Promise<T> => {
    try {
      return (await transport.send(cmd, args)) as T;
    } catch (e) {
      throw toError(cmd, e);
    }
  };
  const nothing = async (cmd: string, args: Record<string, unknown>): Promise<void> => {
    await call(cmd, args);
  };

  const fs: FsApi = {
    list: (path, o = {}) => call<FsList>("os_fs_list", opt({ path, cursor: o.cursor, limit: o.limit })),
    async listAll(path) {
      const out: FsEntry[] = [];
      let cursor: string | undefined;
      do {
        const page = await fs.list(path, { cursor });
        out.push(...page.entries);
        cursor = page.cursor ?? undefined;
      } while (cursor);
      return out;
    },
    stat: (path) => call<FsStat>("os_fs_stat", opt({ path })),
    read: (path, o = {}) => call<FsRead>("os_fs_read", opt({ path, offset: o.offset, length: o.length, as: o.as })),
    async readText(path) {
      let text = "";
      let offset = 0;
      for (;;) {
        const r = await fs.read(path, { offset, as: "text" });
        text += r.data;
        offset = r.offset + r.length;
        if (r.eof || r.length === 0) return text;
      }
    },
    async readBytes(path) {
      const parts: Uint8Array[] = [];
      let offset = 0;
      for (;;) {
        const r = await fs.read(path, { offset, as: "base64" });
        parts.push(fromBase64(r.data));
        offset = r.offset + r.length;
        if (r.eof || r.length === 0) break;
      }
      const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
      let at = 0;
      for (const p of parts) {
        out.set(p, at);
        at += p.length;
      }
      return out;
    },
    write: (path, data, o = {}) => call<FsWrite>("os_fs_write", opt({ path, data, as: o.as, overwrite: o.overwrite })),
    writeText: (path, text, o = {}) => fs.write(path, text, { as: "text", overwrite: o.overwrite }),
    writeBytes: (path, bytes, o = {}) => fs.write(path, toBase64(bytes), { as: "base64", overwrite: o.overwrite }),
    mkdir: (path) => nothing("os_fs_mkdir", { path }),
    rename: (from, to) => nothing("os_fs_rename", { from, to }),
    copy: (from, to) => call<FsCopy>("os_fs_copy", { from, to }),
    trash: (path) => nothing("os_fs_trash", { path }),
    delete: (path, o = {}) => nothing("os_fs_delete", opt({ path, recursive: o.recursive })),
    async watch(path, fn) {
      const { watch } = await call<FsWatch>("os_fs_watch", { path });
      const off = transport.listen((m) => {
        if (m.event === "os_fs_changed" && m.watch === watch) fn(m as unknown as FsChangedEvent);
      });
      return async () => {
        off();
        await nothing("os_fs_unwatch", { watch });
      };
    },
  };

  const store: StoreApi = {
    async get<T = unknown>(key: string): Promise<T | undefined> {
      const v = await call<T | null>("os_store_get", { key });
      return v === null ? undefined : v;
    },
    set: (key, value) => nothing("os_store_set", { key, value: value === undefined ? null : value }),
    delete: (key) => call<boolean>("os_store_delete", { key }),
    list: (prefix) => call<string[]>("os_store_list", opt({ prefix })),
  };

  const windows: WindowsApi = {
    open: (spec) => call<Opened>("os_win_open", spec as unknown as Record<string, unknown>),
    close: (wid) => nothing("os_win_close", { wid }),
    minimize: (wid) => nothing("os_win_minimize", { wid }),
    restore: (wid) => nothing("os_win_restore", { wid }),
    maximize: (wid) => nothing("os_win_maximize", { wid }),
    focus: (wid) => nothing("os_win_focus", { wid }),
    snap: (wid, zone) => nothing("os_win_snap", { wid, zone }),
    set: (wid, patch) => nothing("os_win_set", { wid, ...patch }),
    list: () => call<WindowInfo[]>("os_win_list", {}),
    onEvent: (fn) =>
      transport.listen((m) => {
        if (m.event !== "os_win" || typeof m.wid !== "string") return;
        const { bos: _b, event: _e, id: _i, ...rest } = m;
        fn(rest as unknown as WinEvent);
      }),
  };

  return {
    backend,
    call,
    onEvent: (fn) => transport.listen(fn),
    fs,
    store,
    windows,
    notify: (title, text, o = {}) => nothing("os_notify", opt({ title, text, tag: o.tag, route: o.route })),
    badge: (count, text) => nothing("os_badge", opt({ count, text })),
    pickFolder: (title) => call<string | null>("os_pick_folder", opt({ title })),
    pickFile: (title) => call<string | null>("os_pick_file", opt({ title })),
    openExternal: (url) => nothing("os_open_external", { url }),
  };
}

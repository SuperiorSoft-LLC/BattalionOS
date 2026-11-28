// SPDX-License-Identifier: Apache-2.0
// APP-API-1: the client against a fake bridge (the frame protocol), typed errors, chunked reads, and the local backend.
import { describe, expect, it } from "vitest";
import {
  BadArgsError,
  BosError,
  createBos,
  ExistsError,
  frameTransport,
  LeavesRootError,
  localTransport,
  NotAvailableError,
  NotFoundError,
  PermissionError,
  TimeoutError,
  TooLargeError,
  toError,
  type Transport,
} from "../src/index";

/** A fake OS shell: answers each bridge message with `answer(cmd, args)` (a throw becomes `{ok: false, error}`). */
function fakeShell(answer: (cmd: string, args: Record<string, unknown>) => unknown) {
  const seen: { cmd: string; args: Record<string, unknown> }[] = [];
  let onMsg: ((e: { source?: unknown; data: unknown }) => void) | undefined;
  const parent = {
    postMessage(m: unknown) {
      const d = m as { bos: number; id: string; cmd: string; args: Record<string, unknown> };
      seen.push({ cmd: d.cmd, args: d.args });
      queueMicrotask(() => {
        try {
          onMsg?.({ source: parent, data: { bos: 1, id: d.id, ok: true, result: answer(d.cmd, d.args) } });
        } catch (e) {
          onMsg?.({ source: parent, data: { bos: 1, id: d.id, ok: false, error: (e as Error).message } });
        }
      });
    },
  };
  const self = {
    addEventListener: (_t: "message", fn: (e: { source?: unknown; data: unknown }) => void) => void (onMsg = fn),
    removeEventListener: () => undefined,
  };
  const emit = (data: unknown, source: unknown = parent) => onMsg?.({ source, data });
  return { parent, self, seen, emit };
}

describe("the frame bridge", () => {
  it("sends {bos: 1, id, cmd, args} and resolves with the result", async () => {
    const s = fakeShell((cmd, a) => (cmd === "os_store_get" ? `v:${a.key}` : null));
    const bos = createBos(frameTransport(s.parent, s.self));
    expect(await bos.store.get("k")).toBe("v:k");
    await bos.notify("Hi", "there", { tag: "t" });
    expect(s.seen).toEqual([
      { cmd: "os_store_get", args: { key: "k" } },
      { cmd: "os_notify", args: { title: "Hi", text: "there", tag: "t" } },
    ]);
  });
  it("ignores messages from any window but the parent, and times out", async () => {
    const s = fakeShell(() => {
      throw new Error("never");
    });
    const parent = { postMessage: () => undefined };
    const t = frameTransport(parent, s.self, { timeoutMs: 10 });
    const got: unknown[] = [];
    t.listen((m) => got.push(m));
    s.emit({ bos: 1, event: "os_fs_changed", watch: "w" }, {});
    expect(got).toEqual([]);
    await expect(createBos(t).store.list()).rejects.toBeInstanceOf(TimeoutError);
  });
  it("maps refusals to typed errors", async () => {
    const s = fakeShell((cmd) => {
      if (cmd === "os_fs_read") throw new Error("path refused: '..' [leaves_root]");
      if (cmd === "os_fs_stat") throw new Error("app 'x' has no 'files.read' permission in its manifest, so 'os_fs_stat' is refused");
      if (cmd === "os_store_set") throw new Error("the store would be more than 10 bytes (store.maxBytes) [too_large]");
      throw new Error("'os_frob' is not available to an app's page");
    });
    const bos = createBos(frameTransport(s.parent, s.self));
    const e1 = await bos.fs.read("../x").catch((e) => e);
    expect(e1).toBeInstanceOf(LeavesRootError);
    expect(e1).toBeInstanceOf(BosError);
    expect(e1.code).toBe("leaves_root");
    expect(e1.message).toBe("path refused: '..'");
    const e2 = await bos.fs.stat().catch((e) => e);
    expect(e2).toBeInstanceOf(PermissionError);
    expect(e2.permission).toBe("files.read");
    await expect(bos.store.set("k", "x")).rejects.toBeInstanceOf(TooLargeError);
    await expect(bos.call("os_frob")).rejects.toBeInstanceOf(NotAvailableError);
  });
  it("toError keeps an unknown code and the call", () => {
    const e = toError("os_fs_list", "nope [not_a_folder]");
    expect(e.code).toBe("not_a_folder");
    expect(e.call).toBe("os_fs_list");
    expect(toError("c", "plain").code).toBe("unknown");
  });
  it("readText follows the chunks to eof; listAll follows the cursor", async () => {
    const text = "héllo wörld";
    const s = fakeShell((cmd, a) => {
      if (cmd === "os_fs_read") {
        const off = (a.offset as number) ?? 0;
        const part = text.slice(off, off + 3);
        return { data: part, size: text.length, offset: off, length: part.length, eof: off + part.length >= text.length };
      }
      if (cmd === "os_fs_list") {
        return a.cursor ? { entries: [{ name: "b", kind: "file", size: 1, modified: 1 }], cursor: null } : { entries: [{ name: "a", kind: "dir", size: 0, modified: 1 }], cursor: "1" };
      }
      return null;
    });
    const bos = createBos(frameTransport(s.parent, s.self));
    expect(await bos.fs.readText("f.txt")).toBe(text);
    expect((await bos.fs.listAll()).map((e) => e.name)).toEqual(["a", "b"]);
  });
  it("watch delivers only its own changes and unwatches", async () => {
    const s = fakeShell((cmd) => (cmd === "os_fs_watch" ? { watch: "w1" } : null));
    const bos = createBos(frameTransport(s.parent, s.self));
    const got: unknown[] = [];
    const stop = await bos.fs.watch("docs", (e) => got.push(e.path));
    s.emit({ bos: 1, event: "os_fs_changed", watch: "w2", path: "x", change: "created" });
    s.emit({ bos: 1, event: "os_fs_changed", watch: "w1", path: "docs/a", change: "created" });
    await stop();
    expect(got).toEqual(["docs/a"]);
    expect(s.seen.at(-1)).toEqual({ cmd: "os_fs_unwatch", args: { watch: "w1" } });
  });
  it("window calls and events", async () => {
    const s = fakeShell((cmd) => (cmd === "os_win_list" ? [] : null));
    const bos = createBos(frameTransport(s.parent, s.self));
    await bos.windows.set("w1", { title: "T" });
    await bos.windows.snap("w1", "left");
    expect(await bos.windows.list()).toEqual([]);
    const ev: unknown[] = [];
    bos.windows.onEvent((e) => ev.push(e));
    s.emit({ bos: 1, event: "os_win", id: "app", wid: "w1", change: "closed" });
    expect(ev).toEqual([{ wid: "w1", change: "closed" }]);
    expect(s.seen.slice(0, 2)).toEqual([
      { cmd: "os_win_set", args: { wid: "w1", title: "T" } },
      { cmd: "os_win_snap", args: { wid: "w1", zone: "left" } },
    ]);
  });
});

describe("the local backend", () => {
  const local = (o = {}) => createBos(localTransport({ files: { "notes/a.txt": "alpha", "b.md": "# b" }, ...o }), "local");

  it("lists, reads, writes, renames, copies and deletes under its root", async () => {
    const bos = local();
    expect((await bos.fs.listAll()).map((e) => [e.name, e.kind])).toEqual([["b.md", "file"], ["notes", "dir"]]);
    expect(await bos.fs.readText("notes/a.txt")).toBe("alpha");
    await bos.fs.writeText("notes/c.txt", "gamma");
    await expect(bos.fs.writeText("notes/c.txt", "x", { overwrite: false })).rejects.toBeInstanceOf(ExistsError);
    await bos.fs.rename("notes/c.txt", "c.txt");
    expect((await bos.fs.copy("notes", "notes2")).entries).toBe(2);
    await expect(bos.fs.delete("notes")).rejects.toMatchObject({ code: "not_empty" });
    await bos.fs.delete("notes", { recursive: true });
    await expect(bos.fs.stat("notes")).rejects.toBeInstanceOf(NotFoundError);
    expect((await bos.fs.listAll()).map((e) => e.name)).toEqual(["b.md", "c.txt", "notes2"]);
  });
  it("refuses paths out of the root like the OS", async () => {
    const bos = local();
    for (const p of ["../x", "a/../../x", "/etc/passwd", "C:\\Windows"]) await expect(bos.fs.readText(p)).rejects.toBeInstanceOf(LeavesRootError);
  });
  it("reads in chunks on character boundaries, and bytes round-trip", async () => {
    const bos = createBos(localTransport({ files: { "u.txt": "ééééé" }, readMaxBytes: 3 }));
    expect(await bos.fs.readText("u.txt")).toBe("ééééé");
    const bytes = new Uint8Array([0, 255, 1, 128, 7]);
    await bos.fs.writeBytes("bin", bytes);
    expect(await bos.fs.readBytes("bin")).toEqual(bytes);
    await expect(bos.fs.readText("bin")).rejects.toMatchObject({ code: "not_text" });
  });
  it("store: get/set/delete/list, null deletes, size limit, kept in a Storage", async () => {
    const mem = new Map<string, string>();
    const storage = { getItem: (k: string) => mem.get(k) ?? null, setItem: (k: string, v: string) => void mem.set(k, v) };
    const bos = local({ storage, storageKey: "app-a", storeMaxBytes: 64 });
    expect(await bos.store.get("x")).toBeUndefined();
    await bos.store.set("x", { n: 1 });
    await bos.store.set("y", [1, 2]);
    expect(await bos.store.get<{ n: number }>("x")).toEqual({ n: 1 });
    expect(await bos.store.list()).toEqual(["x", "y"]);
    await bos.store.set("y", null);
    expect(await bos.store.list()).toEqual(["x"]);
    await expect(bos.store.set("z", "q".repeat(100))).rejects.toBeInstanceOf(TooLargeError);
    await expect(bos.store.get("")).rejects.toBeInstanceOf(BadArgsError);
    expect(await bos.store.delete("x")).toBe(true);
    expect(await bos.store.delete("x")).toBe(false);
    // A reload reads it back from the Storage.
    await bos.store.set("kept", 1);
    expect(await local({ storage, storageKey: "app-a" }).store.get("kept")).toBe(1);
  });
  it("two apps (two storage keys) never see each other's store", async () => {
    const mem = new Map<string, string>();
    const storage = { getItem: (k: string) => mem.get(k) ?? null, setItem: (k: string, v: string) => void mem.set(k, v) };
    const a = local({ storage, storageKey: "app-a" });
    const b = local({ storage, storageKey: "app-b" });
    await a.store.set("k", "a");
    expect(await b.store.get("k")).toBeUndefined();
    expect(await b.store.list()).toEqual([]);
  });
  it("watch events in the local tree", async () => {
    const bos = local();
    const got: unknown[] = [];
    const stop = await bos.fs.watch("notes", (e) => got.push([e.path, e.change]));
    await bos.fs.writeText("notes/n.txt", "x");
    await stop();
    await bos.fs.writeText("notes/m.txt", "x");
    expect(got).toEqual([["notes/n.txt", "created"]]);
  });
  it("calls it does not serve are NotAvailable", async () => {
    await expect(local().pickFolder()).rejects.toBeInstanceOf(NotAvailableError);
  });
});

describe("a custom transport", () => {
  it("is all a client needs (another runtime can plug in)", async () => {
    const t: Transport = { send: async (cmd) => (cmd === "os_store_list" ? ["a"] : null), listen: () => () => undefined };
    expect(await createBos(t).store.list()).toEqual(["a"]);
  });
});

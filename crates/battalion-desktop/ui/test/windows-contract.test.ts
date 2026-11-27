// SPDX-License-Identifier: MPL-2.0
// WM-5 (OS-WINDOW-1 §2.4): ONE contract suite for the `windows` client, run against both backends.
//  - local: the in-page window manager (standalone Tauri, the browser).
//  - os: the frame bridge (`{bos:1,id,cmd,args}` to the parent, replies and `os_win` events back), talking to a
//    stand-in OS that answers like battalion-desktop win_calls.rs (and keeps the real kind of a native window).
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Dock, dockAction, drawnBox } from "../kit/blocks/local-desk";
import type { WinCall } from "../kit/lib/windows/backend";
import { createWindows, type Windows } from "../kit/lib/windows/client";
import { LocalWindowManager, type KeyValueStore } from "../kit/lib/windows/local-backend";
import { frameTransport, osBackend } from "../kit/lib/windows/os-backend";
import type { WinEvent } from "../kit/lib/windows/types.gen";

const SURFACE = { w: 1000, h: 800 };
const memory = (): KeyValueStore & { data: Map<string, string> } => {
  const data = new Map<string, string>();
  return { data, getItem: (k) => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v) };
};
const tick = () => new Promise((r) => setTimeout(r, 0));

/** A stand-in BattalionOS shell: serves the bridge messages of one app frame. */
function fakeOs() {
  const wm = new LocalWindowManager({ storage: null, surface: () => SURFACE });
  const pageListeners = new Set<(e: { source?: unknown; data: unknown }) => void>();
  const parent = {
    postMessage(m: unknown) {
      const { id, cmd, args } = m as { id: string; cmd: WinCall; args: Record<string, unknown> };
      void wm.call(cmd, args).then(
        (result) => {
          const r = cmd === "os_win_open" && args.kind === "native" ? { ...(result as object), kind: "native" } : result;
          send({ bos: 1, id, ok: true, result: r });
        },
        (e: Error) => send({ bos: 1, id, ok: false, error: e.message }),
      );
    },
  };
  const send = (data: unknown) => setTimeout(() => pageListeners.forEach((l) => l({ source: parent, data })), 0);
  // The shell forwards os_win to the owning frame (WM-2 winEventFor).
  wm.onEvent((e) => send({ ...e, bos: 1, event: "os_win" }));
  const self = { addEventListener: (_: "message", fn: (e: { source?: unknown; data: unknown }) => void) => void pageListeners.add(fn), removeEventListener: () => {} };
  return { wm, transport: frameTransport(parent, self), injectFromElsewhere: (data: unknown) => pageListeners.forEach((l) => l({ source: {}, data })) };
}

const backends: { name: string; make: () => Windows; nativeKind: string }[] = [
  { name: "local", make: () => createWindows(new LocalWindowManager({ storage: memory(), surface: () => SURFACE })), nativeKind: "inline" },
  { name: "os", make: () => createWindows(osBackend(fakeOs().transport)), nativeKind: "native" },
];

describe.each(backends)("windows contract: $name backend", ({ make, nativeKind }) => {
  it("open gives a handle; the same key focuses the open window instead of a duplicate", async () => {
    const ws = make();
    const a = await ws.open({ key: "session.1", kind: "inline", title: "One" });
    expect(a.wid).toBeTruthy();
    expect([a.kind, a.state, a.restored]).toEqual(["inline", "normal", false]);
    const b = await ws.open({ key: "session.2", title: "Two" });
    const again = await ws.open({ key: "session.1", title: "One" });
    expect(again).toBe(a);
    const list = await ws.list();
    expect(list.map((w) => w.key)).toEqual(["session.2", "session.1"]);
    expect(list.map((w) => w.wid)).toEqual([b.wid, a.wid]);
  });

  it("minimize / restore / maximize / focus answer the state and the handle follows", async () => {
    const ws = make();
    const w = await ws.open({ key: "k", title: "K" });
    const seen: unknown[] = [];
    w.on("state", (s) => seen.push(s.state));
    expect(await w.minimize()).toBe("minimized");
    expect(w.state).toBe("minimized");
    expect(await w.focus()).toBe("normal");
    expect(await w.maximize()).toBe("maximized");
    expect(await w.restore()).toBe("normal");
    await tick();
    expect(seen).toEqual(["minimized", "maximized", "normal"]);
  });

  it("focus raises the window to the top of the z order", async () => {
    const ws = make();
    const a = await ws.open({ key: "a" });
    await ws.open({ key: "b" });
    const focus: number[] = [];
    a.on("focus", (f) => focus.push(f.z));
    await a.focus();
    await tick();
    const list = await ws.list();
    expect(list.at(-1)?.wid).toBe(a.wid);
    expect(focus).toEqual([list.at(-1)?.z]);
  });

  it("snap takes a half or a quarter of the surface", async () => {
    const ws = make();
    const w = await ws.open({ key: "s" });
    expect(await w.snap("left")).toEqual({ state: { snap: "left" }, bounds: { x: 0, y: 0, w: 500, h: 800 } });
    expect(await w.snap("br")).toEqual({ state: { snap: "br" }, bounds: { x: 500, y: 400, w: 500, h: 400 } });
    expect(w.bounds).toEqual({ x: 500, y: 400, w: 500, h: 400 });
  });

  it("set changes title, badge and attention; null clears the badge; bounds move the window", async () => {
    const ws = make();
    const w = await ws.open({ key: "x", title: "Old" });
    await w.set({ title: "New", badge: { count: 2 }, attention: true });
    let row = (await ws.list())[0]!;
    expect([row.title, row.badge, row.attention]).toEqual(["New", { count: 2 }, true]);
    await w.set({ badge: null, bounds: { x: 10, y: 20, w: 400, h: 300 } });
    row = (await ws.list())[0]!;
    expect(row.badge).toBeUndefined();
    expect(row.bounds).toEqual({ x: 10, y: 20, w: 400, h: 300 });
  });

  it("close removes the window and fires closed", async () => {
    const ws = make();
    const w = await ws.open({ key: "c" });
    let closed = 0;
    w.on("closed", () => closed++);
    await w.close();
    await tick();
    expect(w.closed).toBe(true);
    expect(closed).toBe(1);
    expect(await ws.list()).toEqual([]);
  });

  it("a refused call rejects with the reason", async () => {
    const ws = make();
    await expect(ws.open({ key: " " })).rejects.toThrow("os_win_open needs a key");
    const w = await ws.open({ key: "z" });
    await expect(w.snap("middle" as never)).rejects.toThrow("needs a zone");
  });

  it(`native answers the kind actually used (${nativeKind})`, async () => {
    const ws = make();
    const w = await ws.open({ key: "pop", kind: "native", title: "Pop", route: "/pop" });
    expect(w.kind).toBe(nativeKind);
  });
});

describe("local backend", () => {
  it("float and native answer inline", async () => {
    const wm = new LocalWindowManager({ storage: null, surface: () => SURFACE });
    for (const kind of ["float", "native"]) expect(((await wm.call("os_win_open", { key: kind, kind })) as { kind: string }).kind).toBe("inline");
  });

  it("geometry is kept per key in storage; saved state wins over the given bounds; minimized keeps the last open state", async () => {
    const storage = memory();
    const one = createWindows(new LocalWindowManager({ storage, scope: "notes", surface: () => SURFACE }));
    const w = await one.open({ key: "doc", bounds: { x: 0, y: 0, w: 400, h: 300 } });
    await w.snap("right");
    await w.minimize();
    expect([...storage.data.keys()]).toEqual(["bos.windows.notes"]);
    const two = createWindows(new LocalWindowManager({ storage, scope: "notes", surface: () => SURFACE }));
    const back = await two.open({ key: "doc", bounds: { x: 1, y: 1, w: 999, h: 999 } });
    expect([back.restored, back.state, back.bounds]).toEqual([true, { snap: "right" }, { x: 500, y: 0, w: 500, h: 800 }]);
  });

  it("a restored box is clamped onto a smaller surface", async () => {
    const storage = memory();
    storage.setItem("bos.windows.app", JSON.stringify({ k: { bounds: { x: 3000, y: 2000, w: 400, h: 300 }, state: "normal" } }));
    const wm = new LocalWindowManager({ storage, surface: () => SURFACE });
    const o = (await wm.call("os_win_open", { key: "k" })) as { bounds: { x: number; y: number } };
    expect(o.bounds.x).toBeLessThan(SURFACE.w);
    expect(o.bounds.y).toBeLessThan(SURFACE.h);
  });

  it("the dock lists every window; a click raises a covered or minimized one and minimizes the top one", async () => {
    const wm = new LocalWindowManager({ storage: null, surface: () => SURFACE });
    await wm.call("os_win_open", { key: "a", title: "Alpha" });
    await wm.call("os_win_open", { key: "b", title: "Beta" });
    await wm.call("os_win_set", { wid: "w-local-2", badge: { count: 3 } });
    let all = wm.windows();
    const html = renderToStaticMarkup(createElement(Dock, { windows: all, onPick: () => {} }));
    expect(html).toContain("data-bos-dock");
    expect(html).toContain("Alpha");
    expect(html).toContain("Beta (3)");
    const [a, b] = [all[0]!, all[1]!];
    expect(dockAction(b, all)).toBe("os_win_minimize");
    expect(dockAction(a, all)).toBe("os_win_focus");
    await wm.call("os_win_minimize", { wid: b.wid });
    all = wm.windows();
    expect(dockAction(all.find((w) => w.wid === b.wid)!, all)).toBe("os_win_focus");
    expect(dockAction(all.find((w) => w.wid === a.wid)!, all)).toBe("os_win_minimize");
  });

  it("a maximized window is drawn over the whole surface", () => {
    const w = { wid: "w", key: "k", kind: "inline", title: "", state: "maximized", bounds: { x: 5, y: 5, w: 400, h: 300 }, z: 1 } as const;
    expect(drawnBox(w, SURFACE)).toEqual({ x: 0, y: 0, w: 1000, h: 800 });
  });
});

describe("os backend", () => {
  it("drops messages that do not come from the OS shell", async () => {
    const os = fakeOs();
    const ws = createWindows(osBackend(os.transport));
    const w = await ws.open({ key: "k" });
    const seen: WinEvent[] = [];
    ws.backend.onEvent((e) => seen.push(e));
    os.injectFromElsewhere({ bos: 1, event: "os_win", wid: w.wid, change: "closed" });
    await tick();
    expect(seen).toEqual([]);
    expect(w.closed).toBe(false);
  });

  it("an OS-side change (a taskbar click) reaches the handle as a state event", async () => {
    const os = fakeOs();
    const ws = createWindows(osBackend(os.transport));
    const w = await ws.open({ key: "k" });
    const seen: unknown[] = [];
    w.on("state", (s) => seen.push(s.state));
    await os.wm.call("os_win_minimize", { wid: w.wid });
    await tick();
    await tick();
    expect(seen).toEqual(["minimized"]);
    expect(w.state).toBe("minimized");
  });
});

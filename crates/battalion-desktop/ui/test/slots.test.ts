// SPDX-License-Identifier: MPL-2.0
import { describe, expect, it } from "vitest";
import { answer, type FrameBackend } from "../src/frames";
import { NativeCalls, parseSlotEvent, slotChannel, SlotWindows, openOp } from "../src/slots";

const be: FrameBackend = {
  appStatus: async (id) => ({ id, state: "running" }),
  appHealth: async () => null,
  badge: async () => {},
  close: () => {},
  minimize: () => {},
  pickFolder: async () => null,
  pickFile: async () => null,
  openExternal: async () => {},
  loginOpen: async () => {},
  loginClose: async () => {},
  notify: async () => {},
  scanCode: async () => null, win: async () => null,
};

describe("app slots (AND-3)", () => {
  it("the channel exists only where the Android host gives it", () => {
    expect(slotChannel({})).toBeNull();
    expect(slotChannel(undefined)).toBeNull();
    const c = { postMessage: () => {}, onmessage: null };
    expect(slotChannel({ bosSlots: c })).toBe(c);
  });

  it("reads only the host's known events", () => {
    expect(parseSlotEvent('{"ev":"left","id":"a"}')).toEqual({ ev: "left", id: "a" });
    expect(parseSlotEvent('{"ev":"ended","id":"a","reason":"reused","text":"t"}')?.ev).toBe("ended");
    expect(parseSlotEvent('{"ev":"eval","id":"a"}')).toBeNull();
    expect(parseSlotEvent('{"ev":"left"}')).toBeNull();
    expect(parseSlotEvent("not json")).toBeNull();
  });

  it("a slot's request is answered as its own app's, never as another's", async () => {
    const w = new SlotWindows();
    const frames = [
      { id: "a", origin: "http://127.0.0.1:5001", window: w.token("a") },
      { id: "b", origin: "http://127.0.0.1:5002", window: w.token("b") },
    ];
    const ok = await answer(frames, { source: w.token("a"), origin: "http://127.0.0.1:5001", data: { bos: 1, id: "1", cmd: "os_window_info" } }, be);
    expect(ok?.message).toEqual({ bos: 1, id: "1", ok: true, result: { label: "app-a", app: "a" } });
    expect(w.idOf(ok?.to)).toBe("a");
    // The slot of a with b's page origin gets nothing; a asking for b is refused.
    expect(await answer(frames, { source: w.token("a"), origin: "http://127.0.0.1:5002", data: { bos: 1, id: "2", cmd: "os_window_info" } }, be)).toBeNull();
    const other = await answer(frames, { source: w.token("a"), origin: "http://127.0.0.1:5001", data: { bos: 1, id: "3", cmd: "os_app_status", args: { id: "b" } } }, be);
    expect(other?.message).toMatchObject({ ok: false });
  });

  it("a dropped slot gets a new token (an old answer reaches no one)", () => {
    const w = new SlotWindows();
    const t = w.token("a");
    w.drop("a");
    expect(w.idOf(t)).toBeNull();
    expect(w.token("a")).not.toBe(t);
  });
});

describe("openOp (AND-3b)", () => {
  it("marks the url as the app's frame and carries the files the slot serves", () => {
    const files = { root: "/data/os/apps/a/1.0.0", pageDir: "ui", csp: "default-src 'self'" };
    const op = openOp("a", { id: "a", version: "1.0.0", origin: "http://a.bos.localhost", url: "http://a.bos.localhost/ui/index.html", files }, "A", "bos-app");
    expect(op).toEqual({ op: "open", id: "a", url: "http://a.bos.localhost/ui/index.html?bos-app=a", origin: "http://a.bos.localhost", name: "A", files });
  });
  it("sends no files when the host serves the page itself", () => {
    const op = openOp("a", { id: "a", version: "1", origin: "http://127.0.0.1:5", url: "http://127.0.0.1:5/i.html?x=1" }, "A", "f");
    expect(op).toEqual({ op: "open", id: "a", url: "http://127.0.0.1:5/i.html?x=1&f=a", origin: "http://127.0.0.1:5", name: "A" });
  });
});

describe("native calls run by the app's slot (AND-4)", () => {
  const channel = () => {
    const sent: unknown[] = [];
    return { sent, c: { postMessage: (m: string) => void sent.push(JSON.parse(m)), onmessage: null } };
  };
  it("sends the checked call to the app's slot and resolves with the slot's answer", async () => {
    const { sent, c } = channel();
    const n = new NativeCalls();
    const p = n.call(c, "aa", { call: "pick_file", args: { title: "T" } });
    expect(sent).toEqual([{ op: "native", id: "aa", req: "n1", call: "pick_file", args: { title: "T" } }]);
    const ev = parseSlotEvent(JSON.stringify({ ev: "native", id: "aa", req: "n1", ok: true, result: "content://x/1" }));
    expect(ev?.ev).toBe("native");
    expect(n.settle(ev as never)).toBe(true);
    await expect(p).resolves.toBe("content://x/1");
  });
  it("only the asking app's slot settles a call; a refusal is the slot's sentence; an ended slot fails its calls", async () => {
    const { c } = channel();
    const n = new NativeCalls();
    const p1 = n.call(c, "aa", { call: "notify", args: {} });
    expect(n.settle({ id: "bb", req: "n1", ok: true })).toBe(false);
    expect(n.settle({ id: "aa", req: "n1", ok: false, error: "Notifications are turned off for BattalionOS." })).toBe(true);
    await expect(p1).rejects.toThrow("Notifications are turned off");
    const p2 = n.call(c, "aa", { call: "scan_code", args: {} });
    n.drop("aa");
    await expect(p2).rejects.toThrow("the app was closed");
    expect(n.settle({ id: "aa", req: "n2", ok: true })).toBe(false);
  });
});

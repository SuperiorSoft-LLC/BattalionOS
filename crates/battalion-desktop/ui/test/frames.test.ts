// SPDX-License-Identifier: MPL-2.0
import { describe, expect, it } from "vitest";
import { answer, FRAME_COMMANDS, type FrameBackend, type FrameEntry } from "../src/frames";

const winA = { name: "frame-a" };
const winB = { name: "frame-b" };
const stranger = { name: "a window the shell did not make" };
const frames: FrameEntry[] = [
  { id: "app-a", origin: "http://127.0.0.1:5001", window: winA },
  { id: "app-b", origin: "http://127.0.0.1:5002", window: winB },
];

function backend() {
  const calls: string[] = [];
  const be: FrameBackend = {
    appStatus: async (id) => (calls.push(`status ${id}`), { id, state: "running" }),
    appHealth: async (id) => (calls.push(`health ${id}`), "Ok"),
    badge: async (id, count, text) => void calls.push(`badge ${id} ${count ?? "-"} ${text ?? "-"}`),
    close: (id) => void calls.push(`close ${id}`),
    minimize: (id) => void calls.push(`minimize ${id}`),
  };
  return { be, calls };
}
const msg = (cmd: string, args?: unknown, id = "r1") => ({ bos: 1, id, cmd, args });
const from = (source: unknown, origin: string, data: unknown) => ({ source, origin, data });

describe("identity: the app is the frame that sent the message", () => {
  it("serves os_window_info with the label and app of the frame", async () => {
    const { be } = backend();
    const r = await answer(frames, from(winA, "http://127.0.0.1:5001", msg("os_window_info")), be);
    expect(r?.message).toEqual({ bos: 1, id: "r1", ok: true, result: { label: "app-app-a", app: "app-a" } });
    expect(r?.to).toBe(winA);
    expect(r?.origin).toBe("http://127.0.0.1:5001");
  });
  it("a message claiming another id is refused and does nothing", async () => {
    const { be, calls } = backend();
    for (const cmd of ["os_app_status", "os_app_health", "os_close_app_window"]) {
      const r = await answer(frames, from(winA, "http://127.0.0.1:5001", msg(cmd, { id: "app-b" })), be);
      expect(r?.message).toMatchObject({ ok: false });
    }
    expect(calls).toEqual([]);
  });
  it("its own id in the message is allowed and the frame's id is the one used", async () => {
    const { be, calls } = backend();
    await answer(frames, from(winB, "http://127.0.0.1:5002", msg("os_app_status", { id: "app-b" })), be);
    await answer(frames, from(winB, "http://127.0.0.1:5002", msg("os_app_status")), be);
    expect(calls).toEqual(["status app-b", "status app-b"]);
  });
  it("a window the shell did not make gets nothing, not even a refusal", async () => {
    const { be, calls } = backend();
    expect(await answer(frames, from(stranger, "http://127.0.0.1:5001", msg("os_window_info")), be)).toBeNull();
    expect(await answer(frames, from(null, "http://127.0.0.1:5001", msg("os_window_info")), be)).toBeNull();
    expect(calls).toEqual([]);
  });
  it("a frame that is no longer on its own origin gets nothing", async () => {
    const { be, calls } = backend();
    expect(await answer(frames, from(winA, "https://evil.example", msg("os_badge", { count: 3 })), be)).toBeNull();
    expect(await answer(frames, from(winA, "http://127.0.0.1:5002", msg("os_badge", { count: 3 })), be)).toBeNull();
    expect(calls).toEqual([]);
  });
  it("a frame that has no window yet (not loaded) matches nothing", async () => {
    const { be } = backend();
    expect(await answer([{ id: "app-a", origin: "http://127.0.0.1:5001", window: null }], from(null, "http://127.0.0.1:5001", msg("os_window_info")), be)).toBeNull();
  });
});

describe("only today's app commands", () => {
  it("lists exactly the six app commands", () => {
    expect([...FRAME_COMMANDS].sort()).toEqual(["os_app_health", "os_app_status", "os_badge", "os_close_app_window", "os_show_desktop", "os_window_info"]);
  });
  for (const cmd of ["os_list_apps", "os_start_app", "os_stop_app", "os_install_app", "os_remove_app", "os_update_app", "os_rollback_app", "os_open_app_window", "os_exit", "os_catalog", "os_open_login", "os_app_page", "os_frame_badge", "core:window:close", "__proto__", ""]) {
    it(`refuses ${JSON.stringify(cmd)}`, async () => {
      const { be, calls } = backend();
      const r = await answer(frames, from(winA, "http://127.0.0.1:5001", msg(cmd, { id: "app-a" })), be);
      expect(r?.message).toMatchObject({ ok: false, error: expect.stringContaining("not available") });
      expect(calls).toEqual([]);
    });
  }
  it("badge, close, show desktop and status act for the frame's app", async () => {
    const { be, calls } = backend();
    const ask = (cmd: string, args?: unknown) => answer(frames, from(winA, "http://127.0.0.1:5001", msg(cmd, args)), be);
    expect((await ask("os_badge", { count: 4 }))?.message).toMatchObject({ ok: true });
    await ask("os_badge", { text: "new" });
    await ask("os_badge", {});
    await ask("os_show_desktop");
    await ask("os_close_app_window");
    await ask("os_app_health");
    expect(calls).toEqual(["badge app-a 4 -", "badge app-a - new", "badge app-a - -", "minimize app-a", "close app-a", "health app-a"]);
  });
  it("a failing command answers with its error", async () => {
    const { be } = backend();
    be.badge = async () => {
      throw "a badge is a count or a text, not both";
    };
    const r = await answer(frames, from(winA, "http://127.0.0.1:5001", msg("os_badge", { count: 1, text: "x" })), be);
    expect(r?.message).toEqual({ bos: 1, id: "r1", ok: false, error: "a badge is a count or a text, not both" });
  });
  it("things that are not requests get nothing", async () => {
    const { be } = backend();
    for (const data of [null, "hello", 7, { cmd: "os_badge" }, { bos: 2, id: "x", cmd: "os_window_info" }, { bos: 1, id: "has space", cmd: "os_window_info" }, { bos: 1, id: "x" }, { bos: 1, id: "x", cmd: 5 }]) {
      expect(await answer(frames, from(winA, "http://127.0.0.1:5001", data), be)).toBeNull();
    }
  });
});

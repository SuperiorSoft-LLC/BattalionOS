// SPDX-License-Identifier: MPL-2.0
import { describe, expect, it } from "vitest";
import { answer, eventFor, FRAME_COMMANDS, type FrameBackend, type FrameEntry } from "../src/frames";

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
    pickFolder: async (id, title) => (calls.push(`pickFolder ${id} ${title ?? "-"}`), id === "app-a" ? "C:\\work\\proj" : null),
    pickFile: async (id, title) => (calls.push(`pickFile ${id} ${title ?? "-"}`), "C:\\work\\a.txt"),
    openExternal: async (id, url) => {
      calls.push(`open ${id} ${url}`);
      if (id === "app-b") throw new Error("app 'app-b' has no 'network' permission in its manifest, so 'os_open_external' is refused");
    },
    loginOpen: async (id, url) => void calls.push(`loginOpen ${id} ${url}`),
    loginClose: async (id) => void calls.push(`loginClose ${id}`),
    notify: async (id, title, text) => void calls.push(`notify ${id} ${title} ${text ?? "-"}`),
    scanCode: async (id) => (calls.push(`scan ${id}`), "bos-pair:abc"),
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
    expect([...FRAME_COMMANDS].sort()).toEqual(["os_app_health", "os_app_status", "os_badge", "os_close_app_window", "os_login_window_close", "os_login_window_open", "os_notify", "os_open_external", "os_pick_file", "os_pick_folder", "os_scan_code", "os_show_desktop", "os_window_info"]);
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

describe("BOS-25: the native calls of a page", () => {
  const A = "http://127.0.0.1:5001";
  it("pick a folder or a file: the path, for the app of the frame, with its title", async () => {
    const { be, calls } = backend();
    expect((await answer(frames, from(winA, A, msg("os_pick_folder", { title: "Project folder" })), be))?.message).toEqual({ bos: 1, id: "r1", ok: true, result: "C:\\work\\proj" });
    expect((await answer(frames, from(winA, A, msg("os_pick_file")), be))?.message).toMatchObject({ ok: true, result: "C:\\work\\a.txt" });
    expect(calls).toEqual(["pickFolder app-a Project folder", "pickFile app-a -"]);
  });
  it("a cancelled dialog is null, not an error", async () => {
    const { be } = backend();
    expect((await answer(frames, from(winB, "http://127.0.0.1:5002", msg("os_pick_folder")), be))?.message).toMatchObject({ ok: true, result: null });
  });
  it("open a link: allowed passes the url on; the host's refusal (no permission) comes back as an error", async () => {
    const { be, calls } = backend();
    expect((await answer(frames, from(winA, A, msg("os_open_external", { url: "https://example.com/x" })), be))?.message).toMatchObject({ ok: true, result: null });
    const r = await answer(frames, from(winB, "http://127.0.0.1:5002", msg("os_open_external", { url: "https://example.com" })), be);
    expect(r?.message).toMatchObject({ ok: false });
    expect(JSON.stringify(r?.message)).toContain("network");
    expect(calls).toEqual(["open app-a https://example.com/x", "open app-b https://example.com"]);
  });
  it("sign-in window: open and close for the frame's app; a missing url is refused before the host", async () => {
    const { be, calls } = backend();
    expect((await answer(frames, from(winA, A, msg("os_login_window_open", { url: "http://127.0.0.1:9/p" })), be))?.message).toMatchObject({ ok: true });
    expect((await answer(frames, from(winA, A, msg("os_login_window_open", {})), be))?.message).toMatchObject({ ok: false });
    expect((await answer(frames, from(winA, A, msg("os_login_window_close")), be))?.message).toMatchObject({ ok: true });
    expect(calls).toEqual(["loginOpen app-a http://127.0.0.1:9/p", "loginClose app-a"]);
  });
  it("an id in the message is never used: another app is refused and nothing runs", async () => {
    const { be, calls } = backend();
    for (const cmd of ["os_pick_folder", "os_pick_file", "os_open_external", "os_login_window_open", "os_login_window_close"]) {
      const r = await answer(frames, from(winA, A, msg(cmd, { id: "app-b", url: "https://example.com" })), be);
      expect(r?.message).toMatchObject({ ok: false });
    }
    expect(calls).toEqual([]);
  });
  it("a window the shell did not make, or a frame off its origin, gets nothing", async () => {
    const { be, calls } = backend();
    expect(await answer(frames, from(stranger, A, msg("os_pick_folder")), be)).toBeNull();
    expect(await answer(frames, from(winA, "http://evil.example", msg("os_open_external", { url: "https://example.com" })), be)).toBeNull();
    expect(calls).toEqual([]);
  });
  it("other host commands stay refused (update, pairing, runner, cli path)", async () => {
    const { be } = backend();
    for (const cmd of ["update_check", "pairing_set", "ui_runner_set", "cli_path_set", "os_exit", "os_install_app", "login_window_open"]) {
      expect((await answer(frames, from(winA, A, msg(cmd)), be))?.message).toMatchObject({ ok: false });
    }
    expect(FRAME_COMMANDS).toContain("os_login_window_open");
  });
  it("sign-in events go to that app's frame only, at its origin", () => {
    expect(eventFor(frames, "app-b", "os_login_url", { url: "http://127.0.0.1:9/done?code=1" })).toEqual({ to: winB, origin: "http://127.0.0.1:5002", message: { bos: 1, event: "os_login_url", url: "http://127.0.0.1:9/done?code=1" } });
    expect(eventFor(frames, "app-a", "os_login_closed")?.message).toEqual({ bos: 1, event: "os_login_closed" });
    expect(eventFor(frames, "app-c", "os_login_closed")).toBeNull();
  });
});

describe("AND-4: notices and code scans go through the same answer()", () => {
  it("serves os_notify and os_scan_code for the frame's own app", async () => {
    const { be, calls } = backend();
    const n = await answer(frames, from(winA, "http://127.0.0.1:5001", msg("os_notify", { title: "Hi", text: "there", id: "app-a" })), be);
    expect(n?.message).toEqual({ bos: 1, id: "r1", ok: true, result: null });
    const s = await answer(frames, from(winA, "http://127.0.0.1:5001", msg("os_scan_code")), be);
    expect(s?.message).toEqual({ bos: 1, id: "r1", ok: true, result: "bos-pair:abc" });
    expect(calls).toEqual(["notify app-a Hi there", "scan app-a"]);
  });
  it("a device's own refusal wording is used for a command outside the list", async () => {
    const { be } = backend();
    be.refusal = async (_id, cmd) => (cmd.includes("spawn") ? "starting a program is never allowed on a phone" : undefined);
    const s = await answer(frames, from(winA, "http://127.0.0.1:5001", msg("os_process_spawn")), be);
    expect(s?.message).toMatchObject({ ok: false, error: "starting a program is never allowed on a phone" });
    const o = await answer(frames, from(winA, "http://127.0.0.1:5001", msg("os_exit")), be);
    expect(o?.message).toMatchObject({ ok: false, error: "'os_exit' is not available to an app's page" });
  });
  it("refuses a notice without a title and a call for another app", async () => {
    const { be, calls } = backend();
    const n = await answer(frames, from(winA, "http://127.0.0.1:5001", msg("os_notify", {})), be);
    expect(n?.message).toMatchObject({ ok: false, error: "os_notify needs a title" });
    const o = await answer(frames, from(winA, "http://127.0.0.1:5001", msg("os_scan_code", { id: "app-b" })), be);
    expect(o?.message).toMatchObject({ ok: false });
    expect(calls).toEqual([]);
  });
});

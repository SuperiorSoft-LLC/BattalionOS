// SPDX-License-Identifier: MPL-2.0
import { describe, expect, it } from "vitest";
import { answer, closedFrames, eventFor, FRAME_COMMANDS, fsEventFor, openFileEventFor, winEventFor, type FrameBackend, type FrameEntry } from "../src/frames";

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
    notify: async (id, title, text, extra) => void calls.push(`notify ${id} ${title} ${text ?? "-"}${extra?.tag ? ` tag=${extra.tag}` : ""}${extra?.route ? ` route=${extra.route}` : ""}`),
    scanCode: async (id) => (calls.push(`scan ${id}`), "bos-pair:abc"),
    win: async (id, cmd, args) => (calls.push(`win ${id} ${cmd} ${JSON.stringify(args)}`), cmd === "os_win_open" ? { wid: `w-${id}-1`, restored: false } : undefined),
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
    expect([...FRAME_COMMANDS].sort()).toEqual(["os_app_health", "os_app_status", "os_badge", "os_close_app_window", "os_fs_copy", "os_fs_delete", "os_fs_list", "os_fs_mkdir", "os_fs_read", "os_fs_rename", "os_fs_stat", "os_fs_trash", "os_fs_unwatch", "os_fs_watch", "os_fs_write", "os_login_window_close", "os_login_window_open", "os_notify", "os_open_external", "os_open_file_take", "os_open_with", "os_pick_file", "os_pick_folder", "os_scan_code", "os_show_desktop", "os_store_delete", "os_store_get", "os_store_list", "os_store_set", "os_win_close", "os_win_focus", "os_win_list", "os_win_maximize", "os_win_minimize", "os_win_open", "os_win_restore", "os_win_set", "os_win_snap", "os_window_info"]);
  });
  for (const cmd of ["os_frame_win_open", "os_win_exit", "os_list_apps", "os_start_app", "os_stop_app", "os_install_app", "os_remove_app", "os_update_app", "os_rollback_app", "os_open_app_window", "os_exit", "os_catalog", "os_open_login", "os_app_page", "os_frame_badge", "core:window:close", "__proto__", ""]) {
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

describe("window calls (WM-2)", () => {
  it("serve the frame's app, with the page's arguments", async () => {
    const { be, calls } = backend();
    const ask = (cmd: string, args?: unknown) => answer(frames, from(winA, "http://127.0.0.1:5001", msg(cmd, args)), be);
    expect((await ask("os_win_open", { key: "session.1", kind: "inline" }))?.message).toMatchObject({ ok: true, result: { wid: "w-app-a-1", restored: false } });
    expect((await ask("os_win_minimize", { wid: "w-app-a-1" }))?.message).toMatchObject({ ok: true, result: null });
    expect((await ask("os_win_list"))?.message).toMatchObject({ ok: true });
    expect(calls).toEqual(['win app-a os_win_open {"key":"session.1","kind":"inline"}', 'win app-a os_win_minimize {"wid":"w-app-a-1"}', "win app-a os_win_list {}"]);
  });
  it("a page cannot name another app", async () => {
    const { be, calls } = backend();
    const r = await answer(frames, from(winA, "http://127.0.0.1:5001", msg("os_win_open", { id: "app-b", key: "k" })), be);
    expect(r?.message).toMatchObject({ ok: false, error: expect.stringContaining("may not act") });
    expect(calls).toEqual([]);
  });
  it("os_win events go to the owning app's frame only, without the app id", () => {
    const e = winEventFor(frames, { bos: 1, event: "os_win", id: "app-b", wid: "w-app-b-2", change: "closed" });
    expect(e).toEqual({ to: winB, origin: frames.find((f) => f.id === "app-b")!.origin, message: { bos: 1, event: "os_win", wid: "w-app-b-2", change: "closed" } });
    expect(winEventFor(frames, { bos: 1, event: "os_win", id: "nobody", wid: "w-nobody-1", change: "closed" })).toBeNull();
    expect(winEventFor(frames, { bos: 1, event: "os_login_url", id: "app-a", wid: "x" })).toBeNull();
    expect(winEventFor(frames, null)).toBeNull();
  });
  it("BOS-NOTIFY-1: os_notify carries tag and route, and a click is told to that app's frame only", async () => {
    const { be, calls } = backend();
    const n = await answer(frames, from(winA, "http://127.0.0.1:5001", msg("os_notify", { title: "T", body: "B", tag: "ticket:7", route: "board" })), be);
    expect(n?.message).toMatchObject({ ok: true });
    expect(calls).toEqual(["notify app-a T B tag=ticket:7 route=board"]);
    const ev = eventFor(frames, "app-a", "os_notify_click", { tag: "ticket:7", route: "board" });
    expect(ev).toMatchObject({ to: winA, origin: "http://127.0.0.1:5001", message: { bos: 1, event: "os_notify_click", tag: "ticket:7", route: "board" } });
  });
});

describe("FS-API-1 file calls", () => {
  it("go to the backend for the frame's own app, args as sent", async () => {
    const { be, calls } = backend();
    const seen: unknown[] = [];
    be.fs = async (id, cmd, args) => { seen.push([id, cmd, args]); return { entries: [] }; };
    const r = await answer(frames, from(winA, "http://127.0.0.1:5001", msg("os_fs_list", { path: "docs" })), be);
    expect(r?.message).toMatchObject({ ok: true, result: { entries: [] } });
    expect(seen).toEqual([["app-a", "os_fs_list", { path: "docs" }]]);
    expect(calls).toEqual([]);
  });
  it("are refused plainly where the device has none", async () => {
    const { be } = backend();
    delete be.fs;
    const r = await answer(frames, from(winA, "http://127.0.0.1:5001", msg("os_fs_read", { path: "a" })), be);
    expect(r?.message).toMatchObject({ ok: false, error: expect.stringContaining("not available") });
  });
  it("a change event goes to that app's frame only, without the app id", () => {
    const e = fsEventFor(frames, { id: "app-b", watch: "w-1", path: "x/y", change: "created" });
    expect(e?.to).toBe(winB);
    expect(e?.message).toEqual({ bos: 1, event: "os_fs_changed", watch: "w-1", path: "x/y", change: "created" });
    expect(fsEventFor(frames, { id: "nobody", watch: "w-1" })).toBeNull();
    expect(fsEventFor(frames, null)).toBeNull();
  });
});

describe("FS-API-2 file calls, write side", () => {
  it("go to the backend for the frame's own app", async () => {
    const { be } = backend();
    const seen: unknown[] = [];
    be.fs = async (id, cmd, args) => { seen.push([id, cmd, args]); return null; };
    for (const cmd of ["os_fs_write", "os_fs_mkdir", "os_fs_rename", "os_fs_copy", "os_fs_trash", "os_fs_delete"]) {
      const r = await answer(frames, from(winB, "http://127.0.0.1:5002", msg(cmd, { path: "a" })), be);
      expect(r?.message).toMatchObject({ ok: true });
    }
    expect(seen.map((s) => (s as unknown[])[0])).toEqual(Array(6).fill("app-b"));
  });
  it("closedFrames names the apps whose frame went away", () => {
    expect(closedFrames(["a", "b", "c"], ["c", "d"])).toEqual(["a", "b"]);
    expect(closedFrames([], ["a"])).toEqual([]);
  });
});

describe("APP-API-1 store calls", () => {
  it("go to the backend for the frame's own app, never an app the page names", async () => {
    const { be } = backend();
    const seen: unknown[][] = [];
    be.store = async (id, cmd, args) => { seen.push([id, cmd, args]); return cmd === "os_store_get" ? 7 : null; };
    for (const cmd of ["os_store_get", "os_store_set", "os_store_delete", "os_store_list"]) {
      const r = await answer(frames, from(winB, "http://127.0.0.1:5002", msg(cmd, { key: "k", app: "app-a" })), be);
      expect(r?.message).toMatchObject({ ok: true });
    }
    expect(seen.map((s) => s[0])).toEqual(Array(4).fill("app-b"));
  });
  it("are refused when the device has no store backend", async () => {
    const { be } = backend();
    delete be.store;
    const r = await answer(frames, from(winB, "http://127.0.0.1:5002", msg("os_store_get", { key: "k" })), be);
    expect(r?.message).toMatchObject({ ok: false });
  });
});

describe("OPEN-WITH-1: open with", () => {
  it("os_open_with goes to the backend for the frame's own app, with only path and appId", async () => {
    const { be } = backend();
    const seen: unknown[] = [];
    be.openWith = async (id, args) => (seen.push([id, args]), { opened: "notes", path: "a.md" });
    const r = await answer(frames, from(winA, "http://127.0.0.1:5001", msg("os_open_with", { path: "a.md", appId: "notes", extra: 1 })), be);
    expect(r?.message).toEqual({ bos: 1, id: "r1", ok: true, result: { opened: "notes", path: "a.md" } });
    expect(seen).toEqual([["app-a", { path: "a.md", appId: "notes" }]]);
  });
  it("os_open_with without a path, or on a device without it, is refused", async () => {
    const { be } = backend();
    const none = await answer(frames, from(winA, "http://127.0.0.1:5001", msg("os_open_with", { path: "a.md" })), be);
    expect(none?.message).toMatchObject({ ok: false, error: "open with is not available on this device yet" });
    be.openWith = async () => ({});
    const bad = await answer(frames, from(winA, "http://127.0.0.1:5001", msg("os_open_with", {})), be);
    expect(bad?.message).toMatchObject({ ok: false });
  });
  it("os_open_file_take answers the frame's own app's files", async () => {
    const { be } = backend();
    be.openFileTake = async (id) => (id === "app-b" ? ["docs/a.md"] : []);
    const r = await answer(frames, from(winB, "http://127.0.0.1:5002", msg("os_open_file_take")), be);
    expect(r?.message).toMatchObject({ ok: true, result: ["docs/a.md"] });
  });
  it("the open-file event goes to that app's frame only", () => {
    const r = openFileEventFor(frames, { id: "app-b", path: "docs/a.md" });
    expect(r).toEqual({ to: winB, origin: "http://127.0.0.1:5002", message: { bos: 1, event: "os_open_file", path: "docs/a.md" } });
    expect(openFileEventFor(frames, { id: "app-z", path: "x" })).toBeNull();
    expect(openFileEventFor(frames, { id: "app-a" })).toBeNull();
  });
});

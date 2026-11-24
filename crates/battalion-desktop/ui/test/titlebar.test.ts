// SPDX-License-Identifier: MPL-2.0
// BOS-27: the window messages of a page (only the view on screen), and who draws the window's title bar.
import { describe, expect, it } from "vitest";
import { answer, type FrameBackend, type FrameEntry } from "../src/frames";
import type { AppStateName, InstalledApp } from "../src/os";
import { initial, reduce, titleBarOwner, type Action } from "../src/store";

const winA = { n: "a" };
const winB = { n: "b" };
const frames: FrameEntry[] = [
  { id: "a", origin: "http://127.0.0.1:5001", window: winA, onScreen: true },
  { id: "b", origin: "http://127.0.0.1:5002", window: winB, onScreen: false },
];
function backend() {
  const calls: string[] = [];
  const no = async () => {
    throw new Error("not used");
  };
  const be: FrameBackend = {
    appStatus: no, appHealth: no, badge: no, close: () => {}, minimize: () => {}, pickFolder: no, pickFile: no, openExternal: no, loginOpen: no, loginClose: no,
    windowMinimize: async () => void calls.push("minimize"),
    windowToggleMaximize: async () => void calls.push("maximize"),
    windowClose: async () => void calls.push("close"),
    windowStartDrag: async () => void calls.push("drag"),
    windowBar: (id, on) => void calls.push(`bar ${id} ${on}`),
  };
  return { be, calls };
}
const [fa, fb] = frames as [FrameEntry, FrameEntry];
const send = (e: FrameEntry, cmd: string, args?: unknown) => ({ source: e.window, origin: e.origin, data: { bos: 1, id: "r1", cmd, args } });

describe("os_window_* messages", () => {
  it("the app on screen minimizes, toggles maximize, closes and starts a drag of the OS window", async () => {
    const { be, calls } = backend();
    for (const c of ["os_window_minimize", "os_window_toggle_maximize", "os_window_close", "os_window_start_drag"]) {
      expect((await answer(frames, send(fa, c), be))?.message).toMatchObject({ ok: true });
    }
    expect(calls).toEqual(["minimize", "maximize", "close", "drag"]);
  });
  it("an app that is not on screen (hidden view) is refused and nothing happens", async () => {
    const { be, calls } = backend();
    for (const c of ["os_window_minimize", "os_window_toggle_maximize", "os_window_close", "os_window_start_drag", "os_window_bar"]) {
      expect((await answer(frames, send(fb, c), be))?.message).toMatchObject({ ok: false });
    }
    expect(calls).toEqual([]);
  });
  it("an id in the message never selects another app, and a stranger gets nothing", async () => {
    const { be, calls } = backend();
    expect((await answer(frames, send(fa, "os_window_close", { id: "b" }), be))?.message).toMatchObject({ ok: false });
    expect(await answer(frames, { source: { n: "x" }, origin: fa.origin, data: { bos: 1, id: "r1", cmd: "os_window_close" } }, be)).toBeNull();
    expect(calls).toEqual([]);
  });
  it("os_window_bar says the bar of the frame's own app is there (on by default, off with on:false)", async () => {
    const { be, calls } = backend();
    await answer(frames, send(fa, "os_window_bar"), be);
    await answer(frames, send(fa, "os_window_bar", { on: false }), be);
    expect(calls).toEqual(["bar a true", "bar a false"]);
  });
});

describe("who draws the window's title bar", () => {
  const app = (id: string, state: AppStateName, ownTitleBar?: boolean): InstalledApp => ({ id, version: "1", state, channel: "stable", ownTitleBar });
  const at = (apps: InstalledApp[], ...more: Action[]) => [{ type: "installed-loaded", apps } as Action, ...more].reduce(reduce, initial);
  const open = (id: string): Action => ({ type: "view-open", id });

  it("the desktop (no app on screen) has the OS bar", () => {
    expect(titleBarOwner(at([app("a", "running", true)]), { a: true })).toBeNull();
  });
  it("an app that declares its bar and said so owns it", () => {
    expect(titleBarOwner(at([app("a", "running", true)], open("a")), { a: true })).toBe("a");
  });
  it("until the page says so the OS bar stays (a page that hangs at start)", () => {
    expect(titleBarOwner(at([app("a", "running", true)], open("a")), {})).toBeNull();
  });
  it("an app that does not declare the field never owns it, even if its page says so", () => {
    expect(titleBarOwner(at([app("a", "running")], open("a")), { a: true })).toBeNull();
  });
  it("a starting or crashed app has the OS bar", () => {
    expect(titleBarOwner(at([app("a", "starting", true)], open("a")), { a: true })).toBeNull();
    expect(titleBarOwner(at([app("a", "failed", true)], open("a")), { a: true })).toBeNull();
  });
  it("a minimized view hands the bar back to the OS", () => {
    expect(titleBarOwner(at([app("a", "running", true)], open("a"), { type: "view-minimize", id: "a" }), { a: true })).toBeNull();
  });
});

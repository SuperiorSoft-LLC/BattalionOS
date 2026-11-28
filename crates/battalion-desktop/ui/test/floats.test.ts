// SPDX-License-Identifier: MPL-2.0
// WM-8: the float kind drawn by the OS: maximize fills the work area only, minimize keeps it and lists it in the BOS taskbar,
// restore keeps the bounds, a native window is never listed as minimized, and the frame in a float gets only its own app's calls.
import { describe, expect, it } from "vitest";
import { answerFloat, floatBox, floatShown, floatsOf, floatUrl } from "../src/floats";
import type { FrameBackend, FrameEntry } from "../src/frames";
import type { InstalledApp } from "../src/os";
import { barItems, initial, reduce, type Model, type Win } from "../src/store";

const app = (id: string): InstalledApp => ({ id, name: id, version: "1.0.0", state: "running" }) as InstalledApp;
const float = (wid: string, more: Partial<Win> = {}): Win => ({ wid, app: "a", title: wid, state: "normal", z: 0, kind: "float", route: "/s/1", bounds: { x: 100, y: 80, w: 600, h: 400 }, ...more });
const model = (wins: Record<string, Win[]>): Model => {
  let m = reduce(initial, { type: "installed-loaded", apps: Object.keys(wins).map(app) });
  for (const [id, ws] of Object.entries(wins)) m = reduce(m, { type: "wins-loaded", id, wins: ws });
  return m;
};
const WORK = { w: 1600, h: 860 }; // the desktop area above the BOS taskbar

describe("float windows (WM-8)", () => {
  it("maximize fills the work area and never reaches under the taskbar", () => {
    const b = floatBox(float("w-a-1", { state: "maximized" }), WORK);
    expect(b).toEqual({ x: 0, y: 0, w: 1600, h: 860 });
    expect(b.y + b.h).toBeLessThanOrEqual(WORK.h);
  });

  it("a snap takes its half of the work area; a normal float keeps its bounds, kept on the work area", () => {
    expect(floatBox(float("w-a-1", { state: { snap: "right" } }), WORK)).toEqual({ x: 800, y: 0, w: 800, h: 860 });
    expect(floatBox(float("w-a-1"), WORK)).toEqual({ x: 100, y: 80, w: 600, h: 400 });
    const off = floatBox(float("w-a-1", { bounds: { x: 5000, y: 5000, w: 600, h: 400 } }), WORK);
    expect(off.y).toBeLessThan(WORK.h);
  });

  it("minimize hides the float but keeps it, listed under its app's taskbar button; restore brings the same bounds", () => {
    const min = float("w-a-1", { state: "minimized" });
    const m = model({ a: [min, float("w-a-2", { z: 1 })] });
    expect(floatsOf(m).map((w) => w.wid)).toEqual(["w-a-1", "w-a-2"]);
    expect(floatShown(min)).toBe(false);
    const bar = barItems(m).find((t) => t.id === "a")!;
    expect(bar.windows.map((w) => [w.wid, w.minimized])).toEqual([["w-a-1", true], ["w-a-2", false]]);
    expect(floatBox({ ...min, state: "normal" }, WORK)).toEqual({ x: 100, y: 80, w: 600, h: 400 });
  });

  it("a native window minimizes into the OS taskbar, so the BOS taskbar never lists it as minimized", () => {
    const m = model({ a: [float("w-a-1", { kind: "native", state: "minimized" })] });
    expect(barItems(m)[0]!.windows[0]!.minimized).toBe(false);
    expect(floatsOf(m)).toEqual([]);
  });

  it("orders floats of every app by the OS z-order", () => {
    const m = model({ a: [float("w-a-1", { z: 3 })], b: [float("w-b-1", { app: "b", z: 1 }), { ...float("w-b-2", { app: "b", z: 9 }), kind: "inline" }] });
    expect(floatsOf(m).map((w) => w.wid)).toEqual(["w-b-1", "w-a-1"]);
  });

  it("loads the route on the app's own origin only", () => {
    expect(floatUrl("http://127.0.0.1:5000", "/session/1?x=2", "a", "w-a-1")).toBe("http://127.0.0.1:5000/session/1?x=2&bos-app=a&bos-wid=w-a-1");
    expect(floatUrl("http://127.0.0.1:5000", "//evil.example/x", "a", "w-a-1")).toBe("http://127.0.0.1:5000/?bos-app=a&bos-wid=w-a-1");
  });
});

describe("the frame in a float (WM-8)", () => {
  const winA = { tag: "a" }, winB = { tag: "b" };
  const frames: FrameEntry[] = [
    { id: "a", origin: "http://127.0.0.1:5001", window: winA, wid: "w-a-1" },
    { id: "b", origin: "http://127.0.0.1:5002", window: winB, wid: "w-b-1" },
  ];
  const seen: string[] = [];
  const be = {
    appStatus: async (id: string) => ({ id, state: "running" }),
    appHealth: async (id: string) => ({ id, ok: true }),
    win: async (id: string, cmd: string, args: Record<string, unknown>) => { seen.push(`${id}:${cmd}:${String(args.wid)}`); return null; },
    pickFile: async () => "C:/secret",
  } as unknown as FrameBackend;
  const ask = (source: unknown, origin: string, cmd: string, args?: Record<string, unknown>) => answerFloat(frames, { source, origin, data: { bos: 1, id: "r1", cmd, args } }, be);

  it("knows its own window and reads its own app's status", async () => {
    expect((await ask(winA, "http://127.0.0.1:5001", "os_window_info"))!.message).toMatchObject({ ok: true, result: { app: "a", wid: "w-a-1" } });
    expect((await ask(winA, "http://127.0.0.1:5001", "os_app_status"))!.message).toMatchObject({ ok: true, result: { id: "a" } });
    expect((await ask(winA, "http://127.0.0.1:5001", "os_app_status", { id: "b" }))!.message).toMatchObject({ ok: false });
  });

  it("makes window calls for its own app only; nothing wider", async () => {
    await ask(winA, "http://127.0.0.1:5001", "os_win_minimize", { wid: "w-b-1" });
    expect(seen).toEqual(["a:os_win_minimize:w-b-1"]); // the host refuses B's wid for app a (win_calls::authorize_wid)
    expect((await ask(winA, "http://127.0.0.1:5001", "os_win_close", { id: "b", wid: "w-b-1" }))!.message).toMatchObject({ ok: false });
    expect((await ask(winA, "http://127.0.0.1:5001", "os_pick_file"))!.message).toMatchObject({ ok: false, error: "'os_pick_file' is not available in a float window" });
    expect(await ask({ tag: "other" }, "http://127.0.0.1:5001", "os_window_info")).toBeNull();
    expect(await ask(winA, "http://127.0.0.1:5002", "os_window_info")).toBeNull();
  });
});

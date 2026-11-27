// SPDX-License-Identifier: MPL-2.0
// WM-3: the taskbar groups an app's windows under its button (or one button per window when bos.taskbar.groupWindows is off),
// the app button shows the app badge or else the sum of its windows' counts, attention pulses, and a closed window leaves the bar.
import { describe, expect, it } from "vitest";
import type { InstalledApp } from "../src/os";
import { barItems, initial, type Model, reduce, type Win } from "../src/store";

const app = (id: string, badge?: string): InstalledApp => ({ id, name: id, version: "1.0.0", state: "running", ...(badge ? { badge } : {}) }) as InstalledApp;
const win = (wid: string, more: Partial<Win> = {}): Win => ({ wid, app: "a", title: wid, state: "normal", z: 0, ...more });
const model = (apps: InstalledApp[], wins: Win[], groupWindows?: boolean): Model => {
  let m = reduce(initial, { type: "installed-loaded", apps });
  m = reduce(m, { type: "view-open", id: "a" });
  m = reduce(m, { type: "wins-loaded", id: "a", wins });
  return groupWindows === undefined ? m : reduce(m, { type: "group-windows", on: groupWindows });
};

describe("taskbar windows (WM-3)", () => {
  it("groups an app's windows under one button, minimized ones included", () => {
    const items = barItems(model([app("a")], [win("w-a-1"), win("w-a-2", { state: "minimized", z: 1 })]));
    expect(items.map((t) => t.key)).toEqual(["a"]);
    expect(items[0]!.windows.map((w) => [w.wid, w.minimized])).toEqual([["w-a-1", false], ["w-a-2", true]]);
  });

  it("shows one button per window when grouping is off", () => {
    const items = barItems(model([app("a")], [win("w-a-1", { title: "One" }), win("w-a-2", { title: "Two", z: 1 })], false));
    expect(items.map((t) => [t.key, t.wid, t.windows[0]!.title])).toEqual([["a/w-a-1", "w-a-1", "One"], ["a/w-a-2", "w-a-2", "Two"]]);
  });

  it("keeps the plain app button when the app has no windows, grouped or not", () => {
    expect(barItems(model([app("a")], [], false)).map((t) => [t.key, t.windows.length])).toEqual([["a", 0]]);
  });

  it("shows the sum of the windows' counts when the app has no badge of its own", () => {
    const ws = [win("w-a-1", { badge: { count: 2 } }), win("w-a-2", { badge: { count: 3 } }), win("w-a-3", { badge: { text: "new" } })];
    expect(barItems(model([app("a")], ws))[0]!.mark).toBe("5");
    expect(barItems(model([app("a")], [win("w-a-1")]))[0]!.mark).toBeNull();
  });

  it("keeps the app badge (os_badge) over the windows' sum", () => {
    expect(barItems(model([app("a", "9")], [win("w-a-1", { badge: { count: 2 } })]))[0]!.mark).toBe("9");
  });

  it("gives each ungrouped button its own window's badge and attention", () => {
    const items = barItems(model([app("a")], [win("w-a-1", { badge: { count: 4 } }), win("w-a-2", { attention: true, z: 1 })], false));
    expect(items.map((t) => [t.mark, t.attention])).toEqual([["4", false], [null, true]]);
  });

  it("pulses the app button when one of its windows asks for attention", () => {
    expect(barItems(model([app("a")], [win("w-a-1"), win("w-a-2", { attention: true })]))[0]!.attention).toBe(true);
    expect(barItems(model([app("a")], [win("w-a-1")]))[0]!.attention).toBe(false);
  });

  it("removes a closed window's item", () => {
    let m = model([app("a")], [win("w-a-1"), win("w-a-2", { z: 1 })], false);
    m = reduce(m, { type: "win-closed", id: "a", wid: "w-a-1" });
    expect(barItems(m).map((t) => t.key)).toEqual(["a/w-a-2"]);
    m = reduce(m, { type: "win-closed", id: "a", wid: "w-a-2" });
    expect(m.wins).toEqual({});
    expect(barItems(m).map((t) => [t.key, t.windows.length])).toEqual([["a", 0]]);
  });

  it("groups by default", () => expect(barItems(model([app("a")], [win("w-a-1"), win("w-a-2")]))).toHaveLength(1));

  it("WM-7: a native window's memory from os_win_list reaches the window list; unknown stays null", () => {
    const items = barItems(model([app("a")], [win("w-a-1", { memoryMb: 142 }), win("w-a-2", { z: 1 })]));
    expect(items[0]?.windows.map((w) => w.memoryMb)).toEqual([142, null]);
  });
});

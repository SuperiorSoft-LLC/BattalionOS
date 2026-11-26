// SPDX-License-Identifier: MPL-2.0
// AND-6: an ended phone slot stays in the taskbar as ended; the slot limit setting; the sign-in view's events.
import { describe, expect, it } from "vitest";
import { barItems, initial, reduce, type Model } from "../src/store";
import type { InstalledApp } from "../src/os";
import { parseSlotEvent, slotLimit } from "../src/slots";

const app = (id: string): InstalledApp => ({ id, name: id, version: "1.0.0", state: "running" }) as unknown as InstalledApp;
const withApp = (): Model => reduce(reduce(initial, { type: "installed-loaded", apps: [app("a")] }), { type: "view-open", id: "a" });

describe("an ended slot (AND-6)", () => {
  it("stays in the taskbar, marked ended and not running, and is not the active view", () => {
    const m = reduce(withApp(), { type: "view-ended", id: "a" });
    expect(m.views).toEqual([]);
    expect(m.active).toBeNull();
    const b = barItems(m).find((t) => t.id === "a");
    expect(b).toMatchObject({ ended: true, running: false, open: false, active: false });
  });
  it("opens fresh on a tap (view-open clears ended)", () => {
    const m = reduce(reduce(withApp(), { type: "view-ended", id: "a" }), { type: "view-open", id: "a" });
    expect(m.ended).toEqual([]);
    expect(barItems(m).find((t) => t.id === "a")).toMatchObject({ ended: false, running: true, active: true });
  });
  it("leaves the bar when the app stops and is closed", () => {
    const m = reduce(reduce(reduce(withApp(), { type: "view-ended", id: "a" }), { type: "installed-loaded", apps: [{ ...app("a"), state: "stopped" } as InstalledApp] }), { type: "view-close", id: "a" });
    expect(barItems(m).some((t) => t.id === "a")).toBe(false);
  });
  it("a desktop model never has an ended item", () => {
    expect(barItems(withApp()).every((t) => t.ended === false)).toBe(true);
  });
});

describe("apps open at once (AND-6)", () => {
  it("reads the host's setting, clamped to the pool", () => {
    expect(slotLimit({ inUse: 2, max: 4 })).toEqual({ inUse: 2, max: 4 });
    expect(slotLimit({ inUse: 9, max: 4 })).toEqual({ inUse: 4, max: 4 });
    expect(slotLimit({ inUse: 0, max: 4 })).toEqual({ inUse: 1, max: 4 });
    expect(slotLimit({ max: 4 })).toBeNull();
    expect(slotLimit({ inUse: 2 })).toBeNull();
    expect(slotLimit({ inUse: 1.5, max: 4 })).toBeNull();
  });
  it("the slots event carries it", () => {
    const e = parseSlotEvent('{"ev":"slots","id":"","slots":[],"inUse":3,"max":4}');
    expect(e?.ev === "slots" && slotLimit(e)).toEqual({ inUse: 3, max: 4 });
  });
});

describe("the sign-in view's events (AND-6)", () => {
  it("parses login url and closed", () => {
    expect(parseSlotEvent('{"ev":"login","id":"a","url":"https://x.example/cb"}')).toMatchObject({ ev: "login", url: "https://x.example/cb" });
    expect(parseSlotEvent('{"ev":"login","id":"a","closed":true}')).toMatchObject({ ev: "login", closed: true });
  });
});

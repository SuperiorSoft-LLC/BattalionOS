// SPDX-License-Identifier: MPL-2.0
// OS-WINDOW-1 (WM-1): the kit's window geometry matches geometry.rs.
import { describe, expect, it } from "vitest";
import { BAR_PX, GRAB_PX, MIN_W, clampBox, dragBox, resizeBox, snapBox, snapZoneAt } from "../kit/lib/geometry";

describe("window geometry (WM-1)", () => {
  it("clamps onto a smaller surface", () => {
    const c = clampBox({ x: 1800, y: 1000, w: 1200, h: 900 }, { w: 800, h: 600 });
    expect(c.w).toBeLessThanOrEqual(800);
    expect(c.x).toBeLessThanOrEqual(800 - GRAB_PX);
    expect(c.y).toBeLessThanOrEqual(600 - BAR_PX);
  });
  it("drags and resizes", () => {
    const s = { w: 1000, h: 800 };
    expect(dragBox({ x: 10, y: 10, w: 400, h: 300 }, 50, 20, s)).toEqual({ x: 60, y: 30, w: 400, h: 300 });
    expect(resizeBox({ x: 100, y: 100, w: 400, h: 300 }, "w", 300, 0, s)).toEqual({ x: 180, y: 100, w: MIN_W, h: 300 });
  });
  it("snaps to halves and quarters", () => {
    const s = { x: 0, y: 40, w: 1000, h: 800 };
    expect(snapBox("right", s)).toEqual({ x: 500, y: 40, w: 500, h: 800 });
    expect(snapBox("bl", s)).toEqual({ x: 0, y: 440, w: 500, h: 400 });
    expect(snapZoneAt(2, 42, s, 8)).toBe("tl");
    expect(snapZoneAt(998, 400, s, 8)).toBe("right");
    expect(snapZoneAt(500, 400, s, 8)).toBeNull();
  });
});

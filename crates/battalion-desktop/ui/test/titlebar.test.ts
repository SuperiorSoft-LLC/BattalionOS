// SPDX-License-Identifier: MPL-2.0
// BOS-27: the window's edge strips (resize from every edge and corner).
import { describe, expect, it } from "vitest";
import { STRIPS } from "../src/TitleBar";

describe("resize strips", () => {
  it("one strip per edge and corner, each starting its own resize", () => {
    expect(STRIPS.map((s) => s.edge).sort()).toEqual(["East", "North", "NorthEast", "NorthWest", "South", "SouthEast", "SouthWest", "West"]);
  });
  it("the sides keep clear of the corners, and every strip has a resize cursor", () => {
    for (const s of STRIPS) expect(s.cursor).toMatch(/resize$/);
    const north = STRIPS.find((s) => s.edge === "North")!.style;
    const nw = STRIPS.find((s) => s.edge === "NorthWest")!.style;
    expect(north.left).toBe(nw.width);
  });
});

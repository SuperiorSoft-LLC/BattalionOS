// SPDX-License-Identifier: MPL-2.0
import { describe, expect, it } from "vitest";
import { firstRunInstalled, firstRunNoticeText } from "../src/firstRunNotice";

describe("first-run notice", () => {
  it("announces only a run that installed apps", () => {
    expect(firstRunInstalled({ status: "done", ids: ["agent-hub"] })).toEqual(["agent-hub"]);
    for (const status of ["already-done", "nothing", "start", "offline", "failed"]) expect(firstRunInstalled({ status, ids: ["x"] })).toEqual([]);
    expect(firstRunInstalled({ status: "done" })).toEqual([]);
  });
  it("says it in one plain line, with where to remove it", () => {
    expect(firstRunNoticeText(["Agent Hub"])).toBe("Agent Hub is installed with BattalionOS. You can remove it in the Armory.");
    expect(firstRunNoticeText(["A", "B", "C"])).toBe("A, B and C are installed with BattalionOS. You can remove them in the Armory.");
    expect(firstRunNoticeText([])).toBeNull();
  });
});

// SPDX-License-Identifier: MPL-2.0
// AND-6 (f4f965ad): taps before the desktop draws are kept and the newest is replayed.
import { describe, expect, it } from "vitest";
import { appAt, lastTap, parseTaps, takeEarlyTap, type BootChannel } from "../src/earlyTaps";

describe("early taps (AND-6)", () => {
  it("keeps only well formed taps", () => {
    expect(parseTaps([{ x: 1, y: 2, t: 3 }, { x: "a" }, null])).toEqual([{ x: 1, y: 2, t: 3 }]);
    expect(parseTaps(undefined)).toEqual([]);
  });
  it("replays the newest tap of either recorder", () => {
    expect(lastTap([{ x: 1, y: 1, t: 5 }], [{ x: 9, y: 9, t: 7 }])).toEqual({ x: 9, y: 9, t: 7 });
    expect(lastTap([], [])).toBeNull();
  });
  it("finds the app under a point from its icon", () => {
    const icon = { dataset: { app: "page-a" } };
    const doc = { elementFromPoint: () => ({ closest: () => icon }) } as unknown as Document;
    expect(appAt(10, 10, doc)).toBe("page-a");
    expect(appAt(10, 10, { elementFromPoint: () => null } as unknown as Document)).toBeNull();
  });
  it("ends the page recorder and merges the host's taps (splash and blank page)", async () => {
    const sent: string[] = [];
    const c: BootChannel = { onmessage: null, postMessage: (m) => { sent.push(m); queueMicrotask(() => c.onmessage?.({ data: JSON.stringify({ taps: [{ x: 50, y: 60, t: 20 }] }) })); } };
    const w = { __bosEarlyTaps: [{ x: 1, y: 1, t: 10 }], bosBoot: c } as Record<string, unknown>;
    expect(await takeEarlyTap(w)).toEqual({ x: 50, y: 60, t: 20 });
    expect(sent).toEqual([JSON.stringify({ op: "taps" })]);
    expect(w.__bosReady).toBe(true);
    expect(w.__bosEarlyTaps).toEqual([]);
  });
  it("without a host uses the page's taps", async () => {
    expect(await takeEarlyTap({ __bosEarlyTaps: [{ x: 3, y: 4, t: 1 }] })).toEqual({ x: 3, y: 4, t: 1 });
  });
});

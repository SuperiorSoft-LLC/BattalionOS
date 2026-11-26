// SPDX-License-Identifier: MPL-2.0
// BOS-30: the BattalionOS row in the Armory: update only, the mark on the icon, the confirm in words, a plain error with Retry.
import { describe, expect, it } from "vitest";
import type { InstalledApp } from "../src/os";
import { armoryHasUpdate, confirmText, osRowActions, runOsUpdate, type OsPhase, type OsUpdateInfo } from "../src/osUpdate";

const info = (over: Partial<OsUpdateInfo> = {}): OsUpdateInfo => ({ current: "0.1.0", available: { version: "0.2.0", notes: "" }, installing: false, busy: [], running: [], ...over });
const app = (id: string, available?: string): InstalledApp => ({ id, version: "1.0.0", state: "running", channel: "stable", available });

describe("the BattalionOS row", () => {
  it("offers Update to X and nothing else: not Open, not Remove", () => {
    expect(osRowActions(info(), { kind: "idle" })).toEqual(["Update to 0.2.0"]);
    for (const a of osRowActions(info(), { kind: "idle" })) expect(a).not.toMatch(/open|remove/i);
  });
  it("has no button when it is current, while it works", () => {
    expect(osRowActions(info({ available: null }), { kind: "idle" })).toEqual([]);
    expect(osRowActions(info(), { kind: "working" })).toEqual([]);
    expect(osRowActions(info({ installing: true }), { kind: "idle" })).toEqual([]);
  });
  it("marks the Armory icon for an OS update or an app update, and for nothing else", () => {
    expect(armoryHasUpdate([app("a")], null)).toBe(false);
    expect(armoryHasUpdate([app("a")], info({ available: null }))).toBe(false);
    expect(armoryHasUpdate([app("a")], info())).toBe(true);
    expect(armoryHasUpdate([app("a", "2.0.0")], null)).toBe(true);
  });
  it("confirms in words and names an app that is still busy", () => {
    expect(confirmText(info({ running: ["a"] }))).toBe("BattalionOS restarts; every running app stops first and starts again.");
    const t = confirmText(info({ running: ["a", "b"], busy: [{ id: "a", name: "Demo App", busy: 2 }] }));
    expect(t).toContain("Demo App is finishing current work, which would be cut short.");
    expect(t).not.toMatch(/in flight|unit/);
  });
  it("a failed update says why and can be tried again", async () => {
    const seen: OsPhase[] = [];
    let n = 0;
    const api = { osUpdateInstall: async () => { if (n++ === 0) throw new Error("package signature check failed: bad"); } };
    await runOsUpdate(api, (p) => seen.push(p), (e) => (e as Error).message);
    expect(seen).toEqual([{ kind: "working" }, { kind: "failed", error: "package signature check failed: bad" }]);
    await runOsUpdate(api, (p) => seen.push(p), (e) => (e as Error).message);
    expect(seen.at(-1)).toEqual({ kind: "working" });
  });
});

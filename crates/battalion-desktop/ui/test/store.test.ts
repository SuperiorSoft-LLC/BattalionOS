// SPDX-License-Identifier: MPL-2.0
import { describe, expect, it } from "vitest";
import type { AppStateName, Catalog, InstalledApp } from "../src/os";
import { appLabel, appView, browseIds, desktopIds, initial, type Model, offline, reduce, runInstall, showFirstRun, taskItems } from "../src/store";

const app = (id: string, state: AppStateName): InstalledApp => ({ id, version: "1.0.0", state, channel: "stable" });
const cat = (over: Partial<Catalog> = {}): Catalog => ({ apps: [{ id: "notes", source: "s", compatible: true }, { id: "old-one", source: "s", compatible: false }], rejected: [], sourceErrors: [], fromCache: false, ...over });
const withInstalled = (...apps: InstalledApp[]): Model => reduce(initial, { type: "installed-loaded", apps });
const err = (e: unknown) => String(e);

describe("app states", () => {
  const cases: [AppStateName, string][] = [
    ["absent", "available"], ["downloading", "installing"], ["verifying", "installing"], ["installed", "installed"], ["initialising", "installing"], ["stopped", "installed"],
    ["starting", "installed"], ["running", "installed"], ["stopping", "installed"], ["updating", "installing"], ["failed", "failed"], ["removing", "installing"],
  ];
  for (const [state, kind] of cases) it(`${state} reads as ${kind}`, () => expect(appView(withInstalled(app("notes", state)), "notes").kind).toBe(kind));

  it("running, starting and stopping are running; stopped is not", () => {
    for (const s of ["running", "starting", "stopping"] as const) expect(appView(withInstalled(app("notes", s)), "notes")).toMatchObject({ running: true });
    expect(appView(withInstalled(app("notes", "stopped")), "notes")).toMatchObject({ running: false });
  });
  it("a catalog app not installed is available, an incompatible one says so", () => {
    const m = reduce(initial, { type: "catalog-loaded", catalog: cat() });
    expect(appView(m, "notes").kind).toBe("available");
    expect(appView(m, "old-one").kind).toBe("incompatible");
  });
  it("shows the install stage from the stored state", () => {
    const m = reduce(withInstalled(app("notes", "verifying")), { type: "install-started", id: "notes" });
    expect(appView(m, "notes")).toEqual({ kind: "installing", stage: "Verifying" });
  });
});

describe("install flow", () => {
  it("installing, then failed with its error, then retry installs again", () => {
    let m = reduce(initial, { type: "install-started", id: "notes" });
    expect(appView(m, "notes").kind).toBe("installing");
    m = reduce(m, { type: "install-failed", id: "notes", error: "digest mismatch" });
    expect(appView(m, "notes")).toEqual({ kind: "failed", error: "digest mismatch" });
    m = reduce(m, { type: "install-started", id: "notes" });
    expect(appView(m, "notes").kind).toBe("installing");
  });
  it("a successful install ends with the icon on the desktop", async () => {
    let m: Model = reduce(initial, { type: "installed-loaded", apps: [] });
    const ok = await runInstall({ install: async () => {}, listApps: async () => [app("notes", "installed")] }, (a) => { m = reduce(m, a); }, "notes", err);
    expect(ok).toBe(true);
    expect(desktopIds(m)).toEqual(["notes"]);
    expect(m.installs).toEqual({});
  });
  it("a failed install keeps the error for Retry and adds no icon", async () => {
    let m: Model = reduce(initial, { type: "installed-loaded", apps: [] });
    const ok = await runInstall({ install: async () => { throw "network down"; }, listApps: async () => [] }, (a) => { m = reduce(m, a); }, "notes", err);
    expect(ok).toBe(false);
    expect(appView(m, "notes")).toEqual({ kind: "failed", error: "network down" });
    expect(desktopIds(m)).toEqual([]);
  });
  it("an install whose list reload fails still ends the install", async () => {
    let m: Model = initial;
    await runInstall({ install: async () => {}, listApps: async () => { throw "boom"; } }, (a) => { m = reduce(m, a); }, "notes", err);
    expect(m.listError).toBe("boom");
    expect(m.installs).toEqual({});
  });
});

describe("desktop and taskbar", () => {
  it("icons come from the installed list, not absent or removing", () => {
    expect(desktopIds(withInstalled(app("a", "installed"), app("b", "absent"), app("c", "removing"), app("d", "failed")))).toEqual(["a", "d"]);
  });
  it("taskbar shows live apps with a running dot and failed ones with a badge", () => {
    const m = withInstalled(app("a", "running"), app("b", "stopped"), app("c", "failed"), app("d", "starting"));
    expect(taskItems(m)).toEqual([{ id: "a", running: true, badge: false }, { id: "c", running: false, badge: true }, { id: "d", running: false, badge: false }]);
  });
});

describe("first run", () => {
  it("is not shown while loading, shown when empty, hidden after dismiss or an install", () => {
    expect(showFirstRun(initial)).toBe(false);
    const empty = withInstalled();
    expect(showFirstRun(empty)).toBe(true);
    expect(showFirstRun(reduce(empty, { type: "dismiss-first-run" }))).toBe(false);
    expect(showFirstRun(reduce(empty, { type: "install-started", id: "notes" }))).toBe(false);
    expect(showFirstRun(withInstalled(app("a", "installed")))).toBe(false);
    expect(showFirstRun(reduce(initial, { type: "installed-failed", error: "x" }))).toBe(false);
  });
});

describe("offline", () => {
  it("online when the catalog is fresh", () => expect(offline(reduce(initial, { type: "catalog-loaded", catalog: cat() }))).toEqual({ offline: false, reason: null }));
  it("offline when served from the cache", () => expect(offline(reduce(initial, { type: "catalog-loaded", catalog: cat({ fromCache: true, sourceErrors: ["timeout"] }) }))).toEqual({ offline: true, reason: "timeout" }));
  it("offline when nothing can be read", () => {
    const m = reduce(initial, { type: "catalog-failed", error: "no network" });
    expect(offline(m)).toEqual({ offline: true, reason: "no network" });
    expect(m.catalog).toBeNull();
  });
  it("a fresh catalog clears an earlier error", () => expect(offline(reduce(reduce(initial, { type: "catalog-failed", error: "x" }), { type: "catalog-loaded", catalog: cat() })).offline).toBe(false));
  it("a source error with a live catalog is a warning, not offline", () => expect(offline(reduce(initial, { type: "catalog-loaded", catalog: cat({ sourceErrors: ["one source down"] }) }))).toEqual({ offline: false, reason: "one source down" }));
});

describe("browse", () => {
  it("lists apps not installed first", () => {
    const m = reduce(withInstalled(app("notes", "installed")), { type: "catalog-loaded", catalog: cat() });
    expect(browseIds(m)).toEqual(["old-one", "notes"]);
  });
  it("labels from the id", () => expect(appLabel("battalion-notes")).toBe("Battalion Notes"));
});

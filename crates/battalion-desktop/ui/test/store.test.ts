// SPDX-License-Identifier: MPL-2.0
import { describe, expect, it } from "vitest";
import type { AppStateName, Catalog, InstalledApp } from "../src/os";
import { appLabel, appView, availableOf, browseIds, desktopIds, iconOf, initial, labelOf, type Model, offline, previousOf, progressOf, reduce, runInstall, runOp, showFirstRun, taskItems } from "../src/store";

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
    expect(taskItems(m)).toEqual([{ id: "a", running: true, badge: false, mark: null }, { id: "c", running: false, badge: true, mark: null }, { id: "d", running: false, badge: false, mark: null }]);
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

describe("names, icons and versions", () => {
  const named: InstalledApp = { ...app("notes", "installed"), name: "Notes", icon: "data:image/png;base64,AA==", available: "1.2.0", previous: "0.9.0" };
  it("shows the manifest name and icon, then the catalog's, then a label from the id", () => {
    const m = reduce(withInstalled(named), { type: "catalog-loaded", catalog: cat({ apps: [{ id: "other", source: "s", compatible: true, name: "Other App", icon: "https://x.example/i.png" }, { id: "plain", source: "s", compatible: true }] }) });
    expect(labelOf(m, "notes")).toBe("Notes");
    expect(iconOf(m, "notes")).toBe("data:image/png;base64,AA==");
    expect(labelOf(m, "other")).toBe("Other App");
    expect(iconOf(m, "other")).toBe("https://x.example/i.png");
    expect(labelOf(m, "plain")).toBe("Plain");
    expect(iconOf(m, "plain")).toBeNull();
  });
  it("knows the version on offer and the one a rollback goes to", () => {
    const m = withInstalled(named, app("b", "installed"));
    expect([availableOf(m, "notes"), previousOf(m, "notes"), availableOf(m, "b"), previousOf(m, "b")]).toEqual(["1.2.0", "0.9.0", null, null]);
  });
});

describe("progress", () => {
  it("reads a download as a percent and the other steps as a moving bar", () => {
    let m = reduce(initial, { type: "install-started", id: "notes" });
    m = reduce(m, { type: "progress", p: { id: "notes", op: "install", stage: "download", bytes: 5242880, total: 10485760, percent: 50 } });
    expect(progressOf(m, "notes")).toEqual({ text: "Downloading: 5.0 of 10 MB (50%)", percent: 50 });
    expect(appView(m, "notes")).toEqual({ kind: "installing", stage: "Downloading" });
    m = reduce(m, { type: "progress", p: { id: "notes", op: "install", stage: "verify" } });
    expect(progressOf(m, "notes")).toEqual({ text: "Verifying", percent: null });
    expect(progressOf(m, "other")).toBeNull();
  });
  it("is dropped when the action ends or fails", () => {
    const p = { id: "notes", op: "update", stage: "stop" } as const;
    for (const end of [{ type: "op-done", id: "notes" }, { type: "op-failed", id: "notes", op: "update", error: "x" }, { type: "install-done", id: "notes" }] as const) {
      expect(reduce(reduce(initial, { type: "progress", p }), end).progress).toEqual({});
    }
  });
});

describe("update, rollback and remove", () => {
  const upd = (id: string): InstalledApp => ({ ...app(id, "running"), available: "2.0.0" });
  it("a successful update reloads the list and clears the phase", async () => {
    let m: Model = withInstalled(upd("notes"));
    const ok = await runOp({ run: async () => {}, listApps: async () => [{ ...app("notes", "running"), version: "2.0.0" }] }, (a) => { m = reduce(m, a); }, "notes", "update", err);
    expect(ok).toBe(true);
    expect(m.ops).toEqual({});
    expect(m.installed[0]!.version).toBe("2.0.0");
  });
  it("a failed update keeps the Armory's text for Retry and touches no other app", async () => {
    let m: Model = withInstalled(upd("notes"), app("other", "running"));
    const before = m.installed[1];
    const ok = await runOp({ run: async () => { throw "update failed (rolled back): never got ready"; }, listApps: async () => m.installed }, (a) => { m = reduce(m, a); }, "notes", "update", err);
    expect(ok).toBe(false);
    expect(m.ops).toEqual({ notes: { kind: "failed", op: "update", error: "update failed (rolled back): never got ready" } });
    expect(m.installed[1]).toBe(before);
    expect(appView(m, "other").kind).toBe("installed");
    m = reduce(m, { type: "op-started", id: "notes", op: "update" });
    expect(m.ops.notes).toEqual({ kind: "working", op: "update" });
    m = reduce(m, { type: "op-dismissed", id: "notes" });
    expect(m.ops).toEqual({});
  });
  it("a removed app leaves the desktop once the list reloads", async () => {
    let m: Model = withInstalled(app("notes", "stopped"), app("other", "running"));
    await runOp({ run: async () => {}, listApps: async () => [app("other", "running")] }, (a) => { m = reduce(m, a); }, "notes", "remove", err);
    expect(desktopIds(m)).toEqual(["other"]);
  });
  it("an app's own badge shows on its taskbar button", () => {
    const m = withInstalled({ ...app("a", "running"), badge: "3" }, app("b", "running"));
    expect(taskItems(m).map((t) => t.mark)).toEqual(["3", null]);
  });
});

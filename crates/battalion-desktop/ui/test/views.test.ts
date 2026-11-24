// SPDX-License-Identifier: MPL-2.0
// An app opens inside the window: open, minimize and reopen keep the view, close ends it, a killed app says so with Retry, an update.
import { describe, expect, it } from "vitest";
import type { AppStateName, InstalledApp } from "../src/os";
import { areaOf, barItems, initial, type Model, reduce, type Action } from "../src/store";

const app = (id: string, state: AppStateName, version = "1.0.0"): InstalledApp => ({ id, version, state, channel: "stable" });
const run = (m: Model, ...actions: Action[]) => actions.reduce(reduce, m);
const loaded = (...apps: InstalledApp[]): Action => ({ type: "installed-loaded", apps });

describe("open, minimize, reopen, close", () => {
  const base = run(initial, loaded(app("a", "running"), app("b", "running")));

  it("opening fills the app area and adds a taskbar entry that is active", () => {
    const m = run(base, { type: "view-open", id: "a" });
    expect(m.active).toBe("a");
    expect(m.views).toEqual(["a"]);
    expect(barItems(m).find((t) => t.id === "a")).toMatchObject({ open: true, active: true });
    expect(barItems(m).find((t) => t.id === "b")).toMatchObject({ open: false, active: false });
  });
  it("minimize goes back to the desktop and the view is kept; reopening is the same view", () => {
    const m = run(base, { type: "view-open", id: "a" }, { type: "view-minimize", id: "a" });
    expect(m.active).toBeNull();
    expect(m.views).toEqual(["a"]);
    expect(barItems(m).find((t) => t.id === "a")).toMatchObject({ open: true, active: false });
    const again = run(m, { type: "view-open", id: "a" });
    expect(again.views).toEqual(["a"]); // one view, so one frame: it was not reloaded
    expect(again.active).toBe("a");
  });
  it("minimizing another app's view leaves the active one alone", () => {
    const m = run(base, { type: "view-open", id: "a" }, { type: "view-open", id: "b" }, { type: "view-minimize", id: "a" });
    expect(m.active).toBe("b");
    expect(m.views).toEqual(["a", "b"]);
  });
  it("switching shows the other app and keeps both views", () => {
    const m = run(base, { type: "view-open", id: "a" }, { type: "view-open", id: "b" }, { type: "view-open", id: "a" });
    expect(m.active).toBe("a");
    expect(m.views).toEqual(["a", "b"]);
  });
  it("close ends the view only: the app stays running and stays in the taskbar by its dot", () => {
    const m = run(base, { type: "view-open", id: "a" }, { type: "view-close", id: "a" });
    expect(m.views).toEqual([]);
    expect(m.active).toBeNull();
    expect(m.installed.find((x) => x.id === "a")?.state).toBe("running");
    expect(barItems(m).find((t) => t.id === "a")).toMatchObject({ running: true, open: false });
  });
  it("a removed app loses its view", () => {
    const m = run(base, { type: "view-open", id: "a" }, loaded(app("b", "running")));
    expect(m.views).toEqual([]);
    expect(m.active).toBeNull();
  });
});

describe("the app area says what the app is doing", () => {
  const area = (state: AppStateName) => areaOf(run(initial, loaded(app("a", state))), "a");
  it("shows the page while the app runs", () => expect(area("running")).toEqual({ kind: "frame" }));
  it("says starting, without Retry", () => expect(area("starting")).toMatchObject({ kind: "status", text: "Starting…", retry: false }));
  it("a killed or crashed app is stopped or failed, with Retry", () => {
    expect(area("stopped")).toMatchObject({ kind: "status", retry: true });
    expect(area("failed")).toMatchObject({ kind: "status", retry: true });
  });
  it("an update says updating", () => expect(area("updating")).toMatchObject({ kind: "status", text: "Updating…" }));
  it("an app that is gone says so", () => expect(areaOf(initial, "a")).toMatchObject({ kind: "status", retry: false }));

  it("a view of a stopped app stays in the taskbar; other apps and the desktop go on", () => {
    const m = run(initial, loaded(app("a", "running"), app("b", "running")), { type: "view-open", id: "a" }, loaded(app("a", "stopped"), app("b", "running")));
    expect(m.views).toEqual(["a"]);
    expect(barItems(m).map((t) => [t.id, t.open, t.running])).toEqual([["b", false, true], ["a", true, false]]);
    expect(areaOf(m, "b")).toEqual({ kind: "frame" });
  });
  it("after Retry the app runs again and the area is the page", () => {
    const m = run(initial, loaded(app("a", "failed")), { type: "view-open", id: "a" }, loaded(app("a", "running")));
    expect(areaOf(m, "a")).toEqual({ kind: "frame" });
  });
});

// SPDX-License-Identifier: MPL-2.0
// AND-UI-1: the phone layout (picked from the host's platform) and the back gesture.
import { describe, expect, it } from "vitest";
import { BackGuard, backStep, layoutFor } from "../src/phone";

describe("layout", () => {
  it("follows the host's platform", () => {
    expect(layoutFor("android")).toBe("phone");
    expect(layoutFor("ios")).toBe("phone");
    expect(layoutFor("windows")).toBe("desktop");
    expect(layoutFor("linux")).toBe("desktop");
    expect(layoutFor("macos")).toBe("desktop");
  });
  it("the host wins over the user agent; the agent decides only when the host says nothing", () => {
    expect(layoutFor("windows", "Mozilla/5.0 (Linux; Android 15)")).toBe("desktop");
    expect(layoutFor(null, "Mozilla/5.0 (Linux; Android 15)")).toBe("phone");
    expect(layoutFor(null, "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")).toBe("desktop");
    expect(layoutFor(undefined)).toBe("desktop");
  });
});

describe("back", () => {
  it("closes the Armory first, then leaves the app for the desktop, then is Android's own", () => {
    expect(backStep(true, "a")).toBe("close-armory");
    expect(backStep(false, "a")).toBe("to-desktop");
    expect(backStep(false, null)).toBeNull();
    // Back with the Start menu open closes the menu and stays in the OS (it does not fall through to Android's back).
    expect(backStep(false, null, true)).toBe("close-start");
    expect(backStep(true, "a", true)).toBe("close-start");
  });
  it("holds one history entry while something is over the desktop", () => {
    const log: string[] = [];
    const g = new BackGuard({ pushState: () => log.push("push"), back: () => log.push("back") });
    g.sync(false);
    g.sync(true);
    g.sync(true); // switching apps keeps the one entry
    expect(log).toEqual(["push"]);
    expect(g.popped()).toBe(true); // the gesture popped it: the shell goes back
    g.sync(false);
    expect(log).toEqual(["push"]); // nothing left to take off
    expect(g.popped()).toBe(false); // a pop at the desktop is not the shell's
  });
  it("takes its entry off when the shell leaves by itself (a taskbar tap)", () => {
    const log: string[] = [];
    const g = new BackGuard({ pushState: () => log.push("push"), back: () => log.push("back") });
    g.sync(true);
    g.sync(false);
    expect(log).toEqual(["push", "back"]);
    expect(g.popped()).toBe(false);
  });
});

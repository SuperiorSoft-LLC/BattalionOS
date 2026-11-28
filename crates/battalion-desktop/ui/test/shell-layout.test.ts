// SPDX-License-Identifier: MPL-2.0
// BOS-LAYOUT-1: the shell is a column exactly the window's height: title bar, app area (fills, clips), taskbar (keeps its height).
import { describe, expect, it } from "vitest";
import { cleanInset, NO_INSET, SHELL_CLASS, shellStyle } from "../src/shellLayout";

import appSrc from "../src/App.tsx?raw";
import desktopSrc from "../src/Desktop.tsx?raw";
import taskbarSrc from "../src/Taskbar.tsx?raw";

const FILES: Record<string, string> = { "src/App.tsx": appSrc, "src/Desktop.tsx": desktopSrc, "src/Taskbar.tsx": taskbarSrc };
const src = (p: string) => FILES[p]!;
const classes = (s: string) => new Set(s.split(/\s+/));

describe("shell column", () => {
  it("the root is the window's height and never scrolls (no 100vh)", () => {
    const c = classes(SHELL_CLASS);
    for (const k of ["flex", "flex-col", "h-full", "max-h-full", "min-h-0", "overflow-hidden"]) expect(c.has(k), k).toBe(true);
    const app = src("src/App.tsx");
    expect(app).not.toMatch(/h-screen/);
    expect(app).toMatch(/className=\{SHELL_CLASS\}/);
  });
  it("the app area fills and clips; the taskbar keeps its height", () => {
    expect(src("src/Desktop.tsx")).toMatch(/className="relative min-h-0 flex-1 overflow-hidden" role="group" aria-label="Desktop"/);
    expect(src("src/Taskbar.tsx")).toMatch(/className=\{`relative flex shrink-0 items-center[^`]*h-12/);
  });
});

describe("work area inset", () => {
  it("no padding while inside the work area", () => {
    expect(shellStyle(false, NO_INSET, {})).toBeUndefined();
  });
  it("a maximized window is padded by its overflow, so the column fits the work area", () => {
    // 1920x1080 at 100 %, 48 px Windows taskbar: the maximized undecorated window is 1936x1048 at (-8,-8).
    const style = shellStyle(false, { top: 8, right: 8, bottom: 8, left: 8 }, {})!;
    expect(style).toMatchObject({ paddingTop: 8, paddingBottom: 8, paddingLeft: 8, paddingRight: 8, boxSizing: "border-box" });
    // The layout the column gets: title bar 36, taskbar 48 (h-12), the app area takes the rest; nothing taller than the root and the
    // taskbar ends at the work area's bottom (1032), not below it.
    const winTop = -8, winH = 1048, workBottom = 1032, title = 36, bar = 48;
    const content = winH - (style.paddingTop as number) - (style.paddingBottom as number);
    const area = content - title - bar;
    expect(area).toBeGreaterThan(0);
    for (const h of [title, area, bar]) expect(h).toBeLessThanOrEqual(content);
    const barBottom = winTop + (style.paddingTop as number) + title + area + bar;
    expect(barBottom).toBe(workBottom);
  });
  it("a phone keeps its own system-bar insets", () => {
    const p = { paddingTop: "env(safe-area-inset-top)" };
    expect(shellStyle(true, { top: 8, right: 8, bottom: 8, left: 8 }, p)).toBe(p);
  });
  it("a bad host answer is treated as no overflow", () => {
    expect(cleanInset({ top: -3, right: Number.NaN } as never)).toEqual(NO_INSET);
    expect(cleanInset(null)).toEqual(NO_INSET);
  });
});

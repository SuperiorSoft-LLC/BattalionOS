// SPDX-License-Identifier: MPL-2.0
// WM-4 (OS-WINDOW-1 §3): the OS-owned window chrome, its tokens and the os_theme event.
import { createElement, isValidElement, type ReactElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { FRAME_STRIPS, WindowFrame, type WindowFrameProps } from "../kit/blocks/window-frame";
import { applyWinTokens, osThemeEvent, parseOsTheme, WIN_TOKENS, WIN_TOKENS_DARK, WIN_TOKENS_LIGHT, winTokens } from "../kit/lib/win-tokens";
import { themeEvents } from "../src/frames";

const html = (p: Partial<WindowFrameProps> = {}) => renderToStaticMarkup(createElement(WindowFrame, { title: "Notes", ...p }));

/** Every element in the tree WindowFrame returns (it has no hooks of its own, so it can be called directly). */
function walk(n: ReactNode, out: ReactElement[] = []): ReactElement[] {
  if (Array.isArray(n)) n.forEach((c) => walk(c, out));
  else if (isValidElement(n)) {
    out.push(n);
    walk((n.props as { children?: ReactNode }).children, out);
  }
  return out;
}
const tree = (p: Partial<WindowFrameProps>) => walk(WindowFrame({ title: "Notes", ...p }));
const button = (els: ReactElement[], label: string) => els.find((e) => (e.props as { label?: string }).label === label)!.props as { onClick?: () => void };
const part = (els: ReactElement[], name: string) => els.find((e) => (e.props as Record<string, unknown>)["data-win-part"] === name)!.props as Record<string, (...a: unknown[]) => void>;

describe("WindowFrame renders", () => {
  it("title, icon, badge, buttons, content and 8 resize strips", () => {
    const h = html({ icon: createElement("i", { id: "ic" }), badge: createElement("b", null, "3"), children: createElement("p", null, "body") });
    expect(h).toContain(">Notes<");
    expect(h).toContain('id="ic"');
    expect(h).toContain("<b>3</b>");
    expect(h).toContain("<p>body</p>");
    for (const l of ["Minimize", "Maximize", "Close"]) expect(h).toContain(`aria-label="${l}"`);
    expect(h.match(/data-win-resize=/g)).toHaveLength(8);
    expect(FRAME_STRIPS.map((s) => s.edge).sort()).toEqual(["East", "North", "NorthEast", "NorthWest", "South", "SouthEast", "SouthWest", "West"]);
  });
  it("maximized: Restore, no strips; the snap preview only when given", () => {
    const h = html({ maximized: true });
    expect(h).toContain('aria-label="Restore"');
    expect(h).not.toContain("data-win-resize");
    expect(h).not.toContain("snap-preview");
    expect(html({ snapPreview: { x: 0, y: 0, w: 400, h: 300 } })).toContain('data-win-part="snap-preview"');
  });
  it("has no className or style props on the chrome", () => {
    const h = html({ ...({ className: "evil", style: { color: "red" } } as object) });
    expect(h).not.toContain("evil");
    expect(h).not.toContain("color:red");
  });
});

describe("WindowFrame callbacks", () => {
  it("minimize, maximize, close, drag, double click and resize", () => {
    const f = { onMinimize: vi.fn(), onMaximize: vi.fn(), onClose: vi.fn(), onDragStart: vi.fn(), onResizeStart: vi.fn() };
    const els = tree(f);
    button(els, "Minimize").onClick!();
    button(els, "Maximize").onClick!();
    button(els, "Close").onClick!();
    expect(f.onMinimize).toHaveBeenCalledOnce();
    expect(f.onMaximize).toHaveBeenCalledOnce();
    expect(f.onClose).toHaveBeenCalledOnce();
    const title = part(els, "title");
    title.onMouseDown!({ button: 0 });
    title.onMouseDown!({ button: 2 });
    expect(f.onDragStart).toHaveBeenCalledOnce();
    title.onDoubleClick!();
    expect(f.onMaximize).toHaveBeenCalledTimes(2);
    const se = els.find((e) => (e.props as Record<string, unknown>)["data-win-resize"] === "SouthEast")!.props as { onMouseDown: (e: unknown) => void };
    se.onMouseDown({ button: 0 });
    expect(f.onResizeStart).toHaveBeenCalledWith("SouthEast", { button: 0 });
  });
});

describe("theme tokens", () => {
  it("the chrome reads every look from a --bos-win-* token", () => {
    const h = html({ snapPreview: { x: 0, y: 0, w: 1, h: 1 } });
    for (const k of WIN_TOKENS.filter((k) => k !== "--bos-win-title-bg-inactive" && k !== "--bos-win-btn-hover" && k !== "--bos-win-btn-danger" && k !== "--bos-win-focus-ring")) expect(h).toContain(`var(${k}`);
    expect(html({ active: false })).toContain("var(--bos-win-title-bg-inactive");
  });
  it("light and dark sets name every token; the title is 32px", () => {
    for (const set of [WIN_TOKENS_LIGHT, WIN_TOKENS_DARK]) {
      expect(Object.keys(set).sort()).toEqual([...WIN_TOKENS].sort());
      expect(set["--bos-win-title-h"]).toBe("32px");
    }
    expect(winTokens(true)).toEqual(WIN_TOKENS_DARK);
    expect(winTokens(false)).toEqual(WIN_TOKENS_LIGHT);
  });
  it("applyWinTokens sets each token on the element", () => {
    const set: Record<string, string> = {};
    applyWinTokens({ style: { setProperty: (k, v) => void (set[k] = v) } }, WIN_TOKENS_LIGHT);
    expect(set).toEqual(WIN_TOKENS_LIGHT);
  });
});

describe("os_theme event", () => {
  it("shape, and it parses back; anything else is null", () => {
    const ev = osThemeEvent(WIN_TOKENS_DARK);
    expect(ev).toEqual({ bos: 1, event: "os_theme", tokens: WIN_TOKENS_DARK });
    expect(parseOsTheme(ev)).toEqual(WIN_TOKENS_DARK);
    expect(parseOsTheme({ bos: 1, event: "os_login_url" })).toBeNull();
    expect(parseOsTheme({ bos: 1, event: "os_theme", tokens: { "--bos-win-bg": "#000" } })).toBeNull();
  });
  it("goes to every frame that has a window, at that frame's origin", () => {
    const a = {}, b = {};
    const r = themeEvents([{ id: "a", origin: "https://a", window: a }, { id: "b", origin: "https://b", window: b }, { id: "gone", origin: "https://c", window: null }], WIN_TOKENS_LIGHT);
    expect(r.map((x) => [x.id, x.to, x.origin])).toEqual([["a", a, "https://a"], ["b", b, "https://b"]]);
    expect(r[0]!.message).toEqual({ bos: 1, event: "os_theme", tokens: WIN_TOKENS_LIGHT });
  });
});

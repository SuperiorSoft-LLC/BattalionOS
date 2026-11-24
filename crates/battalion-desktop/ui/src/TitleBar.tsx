// SPDX-License-Identifier: MPL-2.0
// The OS window has no Windows title bar: this is its bar, always on top (desktop, Armory, first run, above any open app). The look is
// the old Infantry AI bar (black, thin border, 40px high, 44px buttons), copied here; nothing is imported from an app. The bar is a Tauri
// drag region (drag, double click to maximize). The window edges and corners are thin strips above everything (a frame would swallow the
// pointer), so the window resizes from every side, also where an app's frame reaches the edge.
import { Boxes, Minus, Square, X } from "lucide-react";
import { useEffect, useState } from "react";
import type { OsWindow, ResizeEdge } from "./os";

const btn = "grid h-full w-11 place-items-center text-muted-foreground transition-colors hover:bg-accent hover:text-foreground [&_svg]:size-4";

/** The bar: the BattalionOS mark, the name of the app on screen (if any), and minimize / maximize / close. */
export function TitleBar({ win, appName }: { win: OsWindow; appName: string | null }) {
  return (
    <header data-tauri-drag-region data-testid="os-title-bar" className="flex h-10 shrink-0 select-none items-center gap-2 border-b bg-background pl-3">
      <Boxes className="pointer-events-none size-4 text-danger" aria-hidden />
      <span data-tauri-drag-region className="pointer-events-none text-sm font-semibold">BattalionOS</span>
      {appName && (
        <span data-tauri-drag-region data-testid="os-title-app" className="pointer-events-none truncate text-sm text-muted-foreground">
          / {appName}
        </span>
      )}
      <div data-tauri-drag-region className="h-full min-w-4 flex-1" />
      <div className="flex h-full items-stretch">
        <button type="button" aria-label="Minimize" title="Minimize" className={btn} onClick={() => void win.minimize()}>
          <Minus />
        </button>
        <button type="button" aria-label="Maximize" title="Maximize" className={btn} onClick={() => void win.toggleMaximize()}>
          <Square className="size-3.5!" />
        </button>
        <button type="button" aria-label="Close" title="Close" className={`${btn} hover:bg-danger hover:text-white`} onClick={() => void win.close()}>
          <X />
        </button>
      </div>
    </header>
  );
}

const EDGE = 4;
const CORNER = 10;
/** The eight strips of the window border and what each starts. */
export const STRIPS: { edge: ResizeEdge; style: React.CSSProperties; cursor: string }[] = [
  { edge: "North", style: { top: 0, left: CORNER, right: CORNER, height: EDGE }, cursor: "ns-resize" },
  { edge: "South", style: { bottom: 0, left: CORNER, right: CORNER, height: EDGE }, cursor: "ns-resize" },
  { edge: "West", style: { left: 0, top: CORNER, bottom: CORNER, width: EDGE }, cursor: "ew-resize" },
  { edge: "East", style: { right: 0, top: CORNER, bottom: CORNER, width: EDGE }, cursor: "ew-resize" },
  { edge: "NorthWest", style: { top: 0, left: 0, width: CORNER, height: CORNER }, cursor: "nwse-resize" },
  { edge: "NorthEast", style: { top: 0, right: 0, width: CORNER, height: CORNER }, cursor: "nesw-resize" },
  { edge: "SouthWest", style: { bottom: 0, left: 0, width: CORNER, height: CORNER }, cursor: "nesw-resize" },
  { edge: "SouthEast", style: { bottom: 0, right: 0, width: CORNER, height: CORNER }, cursor: "nwse-resize" },
];

/** Eight strips on the window's border. None while the window is maximized (nothing to resize, and they would cover the bar's buttons). */
export function ResizeEdges({ win }: { win: OsWindow }) {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => win.onMaximized(setMaximized), [win]);
  if (maximized) return null;
  return (
    <>
      {STRIPS.map((s) => (
        <div key={s.edge} data-resize={s.edge} className="fixed z-[100]" style={{ ...s.style, cursor: s.cursor }} onMouseDown={(e) => e.button === 0 && void win.startResize(s.edge)} />
      ))}
    </>
  );
}

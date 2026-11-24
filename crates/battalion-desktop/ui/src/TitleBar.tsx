// SPDX-License-Identifier: MPL-2.0
// BOS-27: the OS window has no Windows title bar. Whenever no app with its own bar is on screen the shell draws this one (name, drag,
// minimize / maximize / close); the window edges and corners are thin strips above everything (a frame would swallow the pointer), so
// the window resizes from every side, also where an app's page reaches the edge.
import { Minus, Square, X } from "lucide-react";
import { useEffect, useState } from "react";
import type { OsWindow, ResizeEdge } from "./os";

const btn = "grid h-8 w-11 place-items-center text-muted-foreground hover:bg-accent hover:text-foreground";

/** The thin OS bar. Drag by its empty parts (Tauri starts the drag and toggles maximize on a double click), buttons stay clickable. */
export function OsTitleBar({ win }: { win: OsWindow }) {
  return (
    <div data-tauri-drag-region data-testid="os-title-bar" className="flex h-8 shrink-0 select-none items-center justify-between border-b bg-panel pl-3 text-xs">
      <span data-tauri-drag-region className="flex-1 truncate py-2 text-muted-foreground">BattalionOS</span>
      <div className="flex">
        <button type="button" aria-label="Minimize" className={btn} onClick={() => void win.minimize()}><Minus className="size-4" /></button>
        <button type="button" aria-label="Maximize" className={btn} onClick={() => void win.toggleMaximize()}><Square className="size-3.5" /></button>
        <button type="button" aria-label="Close" className={`${btn} hover:bg-destructive hover:text-destructive-foreground`} onClick={() => void win.close()}><X className="size-4" /></button>
      </div>
    </div>
  );
}

const EDGE = 4;
const CORNER = 10;
const STRIPS: { edge: ResizeEdge; style: React.CSSProperties; cursor: string }[] = [
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

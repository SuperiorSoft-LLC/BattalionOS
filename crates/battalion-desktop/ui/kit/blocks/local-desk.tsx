// SPDX-License-Identifier: Apache-2.0
// Copyright (c) SuperiorSoft LLC
// WM-5: what the local window backend draws when there is no BattalionOS: each open window in the OS chrome
// (WindowFrame), in z order, and a dock strip along the bottom edge as the taskbar (one button per window; a click
// restores/raises it, or minimizes the top one). State lives in the LocalWindowManager; this only renders it.
import { useEffect, useReducer, useState, type CSSProperties, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import { dragBox, resizeBox, snapBox, snapZoneAt, type Surface } from "../lib/geometry";
import { DOCK_H, type LocalWindowManager } from "../lib/windows/local-backend";
import type { WindowInfo } from "../lib/windows/types.gen";
import { tok, WindowFrame, type FrameRect, type WindowEdge } from "./window-frame";

const SNAP_EDGE = 24;
const EDGE_OF: Record<WindowEdge, string> = { North: "n", South: "s", East: "e", West: "w", NorthEast: "ne", NorthWest: "nw", SouthEast: "se", SouthWest: "sw" };

/** What a dock click asks for: a minimized or covered window comes to the top; the top one is minimized. */
export function dockAction(w: WindowInfo, all: WindowInfo[]): "os_win_focus" | "os_win_minimize" {
  const top = all.filter((x) => x.state !== "minimized").reduce<WindowInfo | null>((a, b) => (a && a.z > b.z ? a : b), null);
  return w.state !== "minimized" && top?.wid === w.wid ? "os_win_minimize" : "os_win_focus";
}

/** Where a window is drawn: the whole surface when maximized, else its bounds. */
export function drawnBox(w: WindowInfo, s: Surface): FrameRect {
  return w.state === "maximized" ? { x: s.x ?? 0, y: s.y ?? 0, w: s.w, h: s.h } : w.bounds;
}

export function Dock({ windows, onPick }: { windows: WindowInfo[]; onPick: (w: WindowInfo) => void }) {
  const bar: CSSProperties = { position: "absolute", left: 0, right: 0, bottom: 0, height: DOCK_H, display: "flex", gap: 4, alignItems: "center", padding: "0 8px", background: tok("--bos-win-title-bg"), borderTop: `1px solid ${tok("--bos-win-border")}`, color: tok("--bos-win-fg") };
  const top = windows.filter((x) => x.state !== "minimized").reduce<number>((a, b) => Math.max(a, b.z), -1);
  return (
    <nav aria-label="windows" data-bos-dock style={bar}>
      {[...windows]
        .sort((a, b) => a.wid.localeCompare(b.wid, undefined, { numeric: true }))
        .map((w) => (
          <button
            key={w.wid}
            type="button"
            data-wid={w.wid}
            data-state={typeof w.state === "string" ? w.state : "snap"}
            aria-pressed={w.z === top && w.state !== "minimized"}
            onClick={() => onPick(w)}
            style={{ maxWidth: 180, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", height: DOCK_H - 10, padding: "0 10px", border: "none", borderRadius: 6, cursor: "pointer", color: "inherit", background: w.z === top && w.state !== "minimized" ? tok("--bos-win-btn-hover") : "transparent", opacity: w.state === "minimized" ? 0.7 : 1, boxShadow: w.attention ? `inset 0 -2px 0 ${tok("--bos-win-btn-danger")}` : undefined }}
          >
            {w.title || w.key}
            {w.badge?.count ? ` (${w.badge.count})` : w.badge?.text ? ` · ${w.badge.text}` : ""}
          </button>
        ))}
    </nav>
  );
}

export interface LocalDeskProps {
  wm: LocalWindowManager;
  /** The content of a window, by its row (the app keys its UI by `w.key`). */
  render: (w: WindowInfo) => ReactNode;
}

export function LocalDesk({ wm, render }: LocalDeskProps) {
  const [, bump] = useReducer((n: number) => n + 1, 0);
  const [preview, setPreview] = useState<FrameRect | null>(null);
  useEffect(() => wm.onChange(bump), [wm]);
  const all = wm.windows();
  const s = wm.surface();
  const top = all.filter((w) => w.state !== "minimized").at(-1);

  const track = (w: WindowInfo, e: ReactMouseEvent, move: (dx: number, dy: number, px: number, py: number) => void, end: (px: number, py: number) => void) => {
    e.preventDefault();
    const x0 = e.clientX;
    const y0 = e.clientY;
    void wm.call("os_win_focus", { wid: w.wid });
    const mm = (m: MouseEvent) => move(m.clientX - x0, m.clientY - y0, m.clientX, m.clientY);
    const mu = (m: MouseEvent) => {
      window.removeEventListener("mousemove", mm);
      window.removeEventListener("mouseup", mu);
      end(m.clientX, m.clientY);
    };
    window.addEventListener("mousemove", mm);
    window.addEventListener("mouseup", mu);
  };

  return (
    <div data-bos-desk style={{ position: "absolute", inset: 0, overflow: "hidden" }}>
      {all
        .filter((w) => w.state !== "minimized")
        .map((w) => {
          const b = drawnBox(w, s);
          return (
            <div key={w.wid} data-wid={w.wid} style={{ position: "absolute", left: b.x, top: b.y, width: b.w, height: b.h, zIndex: w.z }}>
              <WindowFrame
                title={w.title}
                badge={w.badge?.count ?? w.badge?.text}
                active={top?.wid === w.wid}
                maximized={w.state === "maximized"}
                snapPreview={top?.wid === w.wid && preview ? { x: preview.x - b.x, y: preview.y - b.y, w: preview.w, h: preview.h } : null}
                onFocus={() => void wm.call("os_win_focus", { wid: w.wid })}
                onMinimize={() => void wm.call("os_win_minimize", { wid: w.wid })}
                onMaximize={() => void wm.call(w.state === "maximized" ? "os_win_restore" : "os_win_maximize", { wid: w.wid })}
                onClose={() => void wm.call("os_win_close", { wid: w.wid })}
                onDragStart={(e) => {
                  let last = w.bounds;
                  track(
                    w,
                    e,
                    (dx, dy, px, py) => {
                      last = dragBox(w.bounds, dx, dy, s);
                      const z = snapZoneAt(px, py, s, SNAP_EDGE);
                      setPreview(z ? snapBox(z, s) : null);
                      void wm.call("os_win_set", { wid: w.wid, bounds: last });
                    },
                    (px, py) => {
                      setPreview(null);
                      const z = snapZoneAt(px, py, s, SNAP_EDGE);
                      if (z) void wm.call("os_win_snap", { wid: w.wid, zone: z });
                      else if (w.state !== "normal") void wm.call("os_win_restore", { wid: w.wid });
                    },
                  );
                }}
                onResizeStart={(edge, e) =>
                  track(
                    w,
                    e,
                    (dx, dy) => void wm.call("os_win_set", { wid: w.wid, bounds: resizeBox(w.bounds, EDGE_OF[edge], dx, dy, s) }),
                    () => {},
                  )
                }
              >
                {render(w)}
              </WindowFrame>
            </div>
          );
        })}
      <Dock windows={all} onPick={(w) => void wm.call(dockAction(w, all), { wid: w.wid })} />
    </div>
  );
}

// SPDX-License-Identifier: MPL-2.0
// The desktop: one icon per installed app, on the owner's grid (grid.ts). Drag to move (snap, swap), arrows to move the focus,
// Enter or double click to open. Positions are kept in localStorage of the shell window.
import { TriangleAlert } from "lucide-react";
import { useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { cellAt, CELL_H, CELL_W, drop, DRAG_THRESHOLD, GRID_PAD, gridSize, neighbor, parsePositions, place, POSITIONS_KEY, serializePositions, type Cell, type Direction, type GridSize } from "./grid";
import { AppIcon } from "./AppIcon";
import type { AppView } from "./store";

const KEYS: Record<string, Direction> = { ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down" };

export function Desktop({ ids, view, label, icon, onOpen, children }: { ids: string[]; view: (id: string) => AppView; label: (id: string) => string; icon: (id: string) => string | null; onOpen: (id: string) => void; children?: ReactNode }) {
  const root = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState<GridSize>({ cols: 8, rows: 6 });
  const [saved, setSaved] = useState(() => parsePositions(localStorage.getItem(POSITIONS_KEY) ?? ""));
  const [dragging, setDragging] = useState<{ id: string; x: number; y: number } | null>(null);
  const press = useRef<{ id: string; x: number; y: number; moved: boolean } | null>(null);
  const shown = useMemo(() => place(ids, saved, size), [ids, saved, size]);

  useLayoutEffect(() => {
    const measure = () => root.current && setSize(gridSize(root.current.clientWidth, root.current.clientHeight));
    measure();
    const ro = new ResizeObserver(measure);
    if (root.current) ro.observe(root.current);
    return () => ro.disconnect();
  }, []);

  const finish = (e: { clientX: number; clientY: number } | null) => {
    const p = press.current;
    press.current = null;
    setDragging(null);
    if (!p || !p.moved || !e || !root.current) return;
    const r = root.current.getBoundingClientRect();
    const cell = cellAt(e.clientX - r.left, e.clientY - r.top, size);
    if (!cell) return;
    const next = drop(ids, saved, size, p.id, cell);
    setSaved(next);
    localStorage.setItem(POSITIONS_KEY, serializePositions(next));
  };
  const at = (c: Cell) => ({ left: GRID_PAD + c.col * CELL_W, top: GRID_PAD + c.row * CELL_H });

  return (
    <div ref={root} className="relative min-h-0 flex-1 overflow-hidden" role="group" aria-label="Desktop">
      {ids.map((id) => {
        const v = view(id);
        const pos = dragging?.id === id ? { left: dragging.x, top: dragging.y } : at(shown[id]!);
        return (
          <button
            key={id}
            type="button"
            style={{ ...pos, width: CELL_W - 8, height: CELL_H - 8 }}
            className="absolute flex touch-none flex-col items-center gap-1 rounded-lg p-2 text-xs text-foreground outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
            aria-label={`${label(id)}${v.kind === "failed" ? " (failed)" : ""}`}
            onDoubleClick={() => onOpen(id)}
            onKeyDown={(e) => {
              const dir = KEYS[e.key];
              if (e.key === "Enter") return onOpen(id);
              if (!dir) return;
              e.preventDefault();
              const to = neighbor(shown, id, dir);
              if (to) root.current?.querySelector<HTMLElement>(`[data-app="${to}"]`)?.focus();
            }}
            data-app={id}
            onPointerDown={(e) => {
              (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
              press.current = { id, x: e.clientX, y: e.clientY, moved: false };
            }}
            onPointerMove={(e) => {
              const p = press.current;
              if (!p || !root.current) return;
              if (!p.moved && Math.hypot(e.clientX - p.x, e.clientY - p.y) < DRAG_THRESHOLD) return;
              p.moved = true;
              const r = root.current.getBoundingClientRect();
              setDragging({ id, x: e.clientX - r.left - CELL_W / 2, y: e.clientY - r.top - CELL_H / 2 });
            }}
            onPointerUp={(e) => finish(e)}
            onPointerCancel={() => finish(null)}
          >
            <span className="relative grid size-12 place-items-center rounded-xl border bg-card text-muted-foreground">
              <AppIcon icon={icon(id)} />
              {v.kind === "failed" && <TriangleAlert className="absolute -right-1 -top-1 size-4 text-danger" aria-hidden />}
            </span>
            <span className="w-full truncate text-center">{label(id)}</span>
          </button>
        );
      })}
      {children}
    </div>
  );
}

// SPDX-License-Identifier: MPL-2.0
// WM-8 (OS-WINDOW-1 §2.2, owner 11-28): the float layer. Every app's `float` windows, drawn by the OS with WindowFrame over the work
// area (the desktop area: above the app views, never under the taskbar). Each holds a frame of its app at its route on the app's own
// origin, so it shares that app's renderer. The OS owns drag, resize, snap, z-order, maximize, close and minimize: a minimized float
// is hidden but kept alive (its frame stays loaded) and listed under its app's taskbar button; restore brings it back at its bounds.
// The title bar's "Pop out to its own window" turns it into a native window (same key and route).
import { ExternalLink } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import { ChromeButton, WindowFrame, type WindowEdge } from "../kit/blocks/window-frame";
import { dragBox, resizeBox, snapBox, snapZoneAt, type Box } from "../kit/lib/geometry";
import { winTokens } from "../kit/lib/win-tokens";
import { themeEvents, winEventFor, type FrameBackend, type FrameEntry } from "./frames";
import { answerFloat, floatBox, floatShown, floatsOf, floatUrl } from "./floats";
import { errText, type AppPage, type Os } from "./os";
import type { Model, Win } from "./store";

const SNAP_EDGE = 24;
const EDGE_OF: Record<WindowEdge, string> = { North: "n", South: "s", East: "e", West: "w", NorthEast: "ne", NorthWest: "nw", SouthEast: "se", SouthWest: "sw" };

export function FloatWindows({ m, os, changed, notify }: { m: Model; os: Os; changed: (id: string) => void; notify?: (text: string) => void }) {
  const floats = floatsOf(m);
  const root = useRef<HTMLDivElement>(null);
  const [work, setWork] = useState({ w: 1200, h: 800 });
  const [pages, setPages] = useState<Record<string, AppPage>>({});
  // While the pointer drags or resizes one float: its live box (sent to the WM on release) and the snap preview.
  const [live, setLive] = useState<{ wid: string; box: Box } | null>(null);
  const [preview, setPreview] = useState<Box | null>(null);
  const frames = useRef<Record<string, Window | null>>({});

  useEffect(() => {
    const el = root.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ob = new ResizeObserver(() => setWork({ w: el.clientWidth, h: el.clientHeight }));
    ob.observe(el);
    setWork({ w: el.clientWidth, h: el.clientHeight });
    return () => ob.disconnect();
  }, []);

  // The page origin of every app that has a float.
  const apps = [...new Set(floats.map((w) => w.app))].join(",");
  useEffect(() => {
    for (const id of apps ? apps.split(",") : []) {
      if (pages[id]) continue;
      os.appPage(id).then((p) => setPages((s) => ({ ...s, [id]: p })), (e) => notify?.(errText(e)));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [apps, os]);

  const entries = useCallback((): FrameEntry[] => floatsOf(mRef.current).flatMap((w) => (pages[w.app] ? [{ id: w.app, origin: pages[w.app]!.origin, window: frames.current[w.wid] ?? null, wid: w.wid }] : [])), [pages]);
  const mRef = useRef(m);
  mRef.current = m;

  const call = useCallback(
    (app: string, cmd: string, args: Record<string, unknown>) =>
      os.frameWin(app, cmd, args).then(
        (r) => { changed(app); return r; },
        (e) => { notify?.(errText(e)); throw e; },
      ),
    [os, changed, notify],
  );

  // The frame in a float: window calls, its window info, its own app's status and health (floats.ts), answered here.
  const backend: FrameBackend = {
    appStatus: (id) => os.appStatus(id),
    appHealth: (id) => os.appHealth(id),
    badge: async () => {},
    close: () => {},
    minimize: () => {},
    pickFolder: async () => null,
    pickFile: async () => null,
    openExternal: async () => {},
    loginOpen: async () => {},
    loginClose: async () => {},
    notify: async () => {},
    scanCode: async () => null,
    win: (id, cmd, args) => os.frameWin(id, cmd, args).then((r) => { if (cmd !== "os_win_list") changed(id); return r; }),
  };
  const beRef = useRef(backend);
  beRef.current = backend;
  useEffect(() => {
    const on = (ev: MessageEvent) => {
      const fs = entries();
      if (!fs.some((f) => f.window != null && f.window === ev.source)) return;
      void answerFloat(fs, { source: ev.source, origin: ev.origin, data: ev.data }, beRef.current).then((r) => {
        if (r) (r.to as Window).postMessage(r.message, r.origin);
      });
    };
    window.addEventListener("message", on);
    return () => window.removeEventListener("message", on);
  }, [entries]);
  // Window events of an app reach its floats' frames too.
  useEffect(
    () =>
      os.onWin((payload) => {
        const p = payload as { id?: string };
        for (const f of entries().filter((e) => e.id === p.id)) {
          const r = winEventFor([f], payload);
          if (r) (r.to as Window).postMessage(r.message, r.origin);
        }
      }),
    [os, entries],
  );

  const onFrame = (w: Win, el: HTMLIFrameElement | null) => {
    const win = el?.contentWindow ?? null;
    if (frames.current[w.wid] === win) return;
    frames.current[w.wid] = win;
    const page = pages[w.app];
    const dark = typeof document === "undefined" || document.documentElement.classList.contains("dark");
    if (win && page) for (const r of themeEvents([{ id: w.app, origin: page.origin, window: win }], winTokens(dark))) win.postMessage(r.message, r.origin);
  };

  const track = (w: Win, e: ReactMouseEvent, move: (dx: number, dy: number, px: number, py: number) => Box, end: (box: Box, px: number, py: number) => void) => {
    e.preventDefault();
    const x0 = e.clientX, y0 = e.clientY;
    const r = root.current?.getBoundingClientRect();
    const ox = r?.left ?? 0, oy = r?.top ?? 0;
    let last = floatBox(w, work);
    const mm = (ev: MouseEvent) => {
      last = move(ev.clientX - x0, ev.clientY - y0, ev.clientX - ox, ev.clientY - oy);
      setLive({ wid: w.wid, box: last });
    };
    const mu = (ev: MouseEvent) => {
      window.removeEventListener("mousemove", mm);
      window.removeEventListener("mouseup", mu);
      setPreview(null);
      end(last, ev.clientX - ox, ev.clientY - oy);
    };
    window.addEventListener("mousemove", mm);
    window.addEventListener("mouseup", mu);
  };
  const moved = (w: Win, box: Box) => {
    const done = () => setLive(null);
    if (!os.shellWinBounds) return done();
    void os.shellWinBounds(w.app, w.wid, box).then(() => changed(w.app), (e) => notify?.(errText(e))).finally(done);
  };

  const shown = floats.filter(floatShown);
  const top = shown.at(-1);
  return (
    <div ref={root} className="pointer-events-none absolute inset-0 z-[8] overflow-hidden" data-testid="float-layer">
      {floats.map((w) => {
        const page = pages[w.app];
        const box = live?.wid === w.wid ? live.box : floatBox(w, work);
        const maximized = w.state === "maximized";
        const dragging = live !== null;
        return (
          <div key={w.wid} data-float={w.wid} data-app={w.app} className={`pointer-events-auto absolute ${floatShown(w) ? "" : "hidden"}`} style={{ left: box.x, top: box.y, width: box.w, height: box.h, zIndex: w.z ?? 0 }}>
            <WindowFrame
              title={w.title || w.app}
              badge={w.badge?.count ?? w.badge?.text}
              active={top?.wid === w.wid}
              maximized={maximized}
              snapPreview={top?.wid === w.wid && preview ? { x: preview.x - box.x, y: preview.y - box.y, w: preview.w, h: preview.h } : null}
              actions={
                os.shellWinPopOut ? (
                  <ChromeButton label="Pop out to its own window" onClick={() => void os.shellWinPopOut!(w.app, w.wid).then(() => changed(w.app), (e) => notify?.(errText(e)))}>
                    <ExternalLink size={14} />
                  </ChromeButton>
                ) : undefined
              }
              onFocus={() => { if (top?.wid !== w.wid) void call(w.app, "os_win_focus", { wid: w.wid }).catch(() => {}); }}
              onMinimize={() => void call(w.app, "os_win_minimize", { wid: w.wid }).catch(() => {})}
              onMaximize={() => void call(w.app, maximized ? "os_win_restore" : "os_win_maximize", { wid: w.wid }).catch(() => {})}
              onClose={() => void call(w.app, "os_win_close", { wid: w.wid }).catch(() => {})}
              onDragStart={(e) => {
                const start = floatBox(w, work);
                track(
                  w,
                  e,
                  (dx, dy, px, py) => {
                    const z = snapZoneAt(px, py, work, SNAP_EDGE);
                    setPreview(z ? snapBox(z, work) : null);
                    return dragBox(start, dx, dy, work);
                  },
                  (b, px, py) => {
                    const z = snapZoneAt(px, py, work, SNAP_EDGE);
                    if (z) void call(w.app, "os_win_snap", { wid: w.wid, zone: z }).catch(() => {}).finally(() => setLive(null));
                    else if (b.x !== start.x || b.y !== start.y || w.state !== "normal") moved(w, b);
                    else setLive(null);
                  },
                );
              }}
              onResizeStart={(edge, e) => {
                const start = floatBox(w, work);
                track(w, e, (dx, dy) => resizeBox(start, EDGE_OF[edge], dx, dy, work), (b) => moved(w, b));
              }}
            >
              {page && (
                <iframe
                  ref={(el) => onFrame(w, el)}
                  title={w.title || w.app}
                  src={floatUrl(page.origin, w.route, w.app, w.wid)}
                  data-float-frame={w.wid}
                  className="absolute inset-0 size-full border-0 bg-background"
                  style={{ pointerEvents: dragging ? "none" : undefined }}
                  allow="clipboard-read; clipboard-write"
                />
              )}
            </WindowFrame>
          </div>
        );
      })}
    </div>
  );
}

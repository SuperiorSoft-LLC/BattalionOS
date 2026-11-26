// SPDX-License-Identifier: MPL-2.0
// The app area: one frame per open app, drawn from the app's own page origin. A frame is hidden (never reloaded) on minimize or switch
// and removed only by Close. The page talks to the OS by messages (frames.ts); the app is the frame that sent it. While the app is not
// running the area says so, with Retry, and the frame loads again once it runs; an update loads the new version.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "../kit";
import { answer, eventFor, type FrameBackend, type FrameEntry } from "./frames";
import type { AppPage, Os } from "./os";
import { errText } from "./os";
import { areaOf, labelOf, type Model } from "./store";
import { openOp, parseSlotEvent, send, slotChannel, SlotWindows, type SlotChannel } from "./slots";

/** The query the shell adds to a page's url so the page knows it is shown inside the OS window (the frame is what the OS trusts, not this). */
export const FRAME_FLAG = "bos-app";

function Frame({ id, page, reload, active, onWindow }: { id: string; page: AppPage; reload: number; active: boolean; onWindow: (id: string, w: Window | null) => void }) {
  const ref = useRef<HTMLIFrameElement>(null);
  const url = `${page.url}${page.url.includes("?") ? "&" : "?"}${FRAME_FLAG}=${encodeURIComponent(id)}`;
  useEffect(() => {
    onWindow(id, ref.current?.contentWindow ?? null);
    return () => onWindow(id, null);
  }, [id, onWindow, url, reload]);
  return (
    <iframe
      ref={ref}
      key={`${page.version}:${reload}`}
      title={id}
      src={url}
      data-app={id}
      className={`absolute inset-0 size-full border-0 bg-background ${active ? "" : "hidden"}`}
      allow="clipboard-read; clipboard-write"
      onLoad={() => onWindow(id, ref.current?.contentWindow ?? null)}
    />
  );
}

export function AppFrames({ m, os, close, minimize, retry, notify, slots: given }: { m: Model; os: Os; close: (id: string) => void; minimize: (id: string) => void; retry: (id: string) => void; notify?: (text: string) => void; slots?: SlotChannel | null }) {
  // AND-3: on Android every app's page runs in its own process slot (the host's channel); everywhere else in a frame here.
  const slots = useMemo(() => (given === undefined ? slotChannel() : given), [given]);
  const slotWin = useRef(new SlotWindows());
  const [pages, setPages] = useState<Record<string, AppPage>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [tick, setTick] = useState(0);
  const [reloads, setReloads] = useState<Record<string, number>>({});
  const windows = useRef<Record<string, Window | null>>({});
  const was = useRef<Record<string, boolean>>({});
  const onWindow = useCallback((id: string, w: Window | null) => void (windows.current[id] = w), []);

  // The page of every view: fetched when the view opens, and again when the app's version changes (an update).
  const versions = m.views.map((id) => `${id}@${m.installed.find((a) => a.id === id)?.version ?? ""}`).join(",");
  useEffect(() => {
    let live = true;
    for (const id of m.views) {
      const v = m.installed.find((a) => a.id === id)?.version;
      if (!v || pages[id]?.version === v) continue;
      os.appPage(id).then(
        (p) => {
          if (!live) return;
          setPages((s) => ({ ...s, [id]: p }));
          setErrors(({ [id]: _e, ...rest }) => rest);
        },
        (e) => live && setErrors((s) => ({ ...s, [id]: errText(e) })),
      );
    }
    return () => void (live = false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [versions, os, tick]);

  // A view whose app stopped or crashed loads its page again when the app runs again.
  useEffect(() => {
    for (const id of m.views) {
      const running = areaOf(m, id).kind === "frame";
      if (running && was.current[id] === false) setReloads((s) => ({ ...s, [id]: (s[id] ?? 0) + 1 }));
      was.current[id] = running;
    }
    for (const id of Object.keys(was.current)) if (!m.views.includes(id)) delete was.current[id];
  }, [m]);

  // A killed app that the OS restarts at once never shows a stopped state: its process is what changed. While a view is open the shell
  // reads the app's status every 2 seconds and loads the page again when the process behind it is another one.
  const proc = useRef<Record<string, string>>({});
  useEffect(() => {
    if (m.views.length === 0) return;
    let live = true;
    const look = () =>
      m.views.forEach((id) =>
        os.appStatus(id).then(
          (s) => {
            const st = s as { state?: string; pid?: number | null; url?: string | null };
            if (!live || st.state !== "running") return;
            const key = `${st.pid ?? ""}@${st.url ?? ""}`;
            const before = proc.current[id];
            proc.current[id] = key;
            if (before && before !== key) setReloads((r) => ({ ...r, [id]: (r[id] ?? 0) + 1 }));
          },
          () => {},
        ),
      );
    look();
    const t = setInterval(look, 2000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [m.views.join(","), os]);

  // A view that was closed leaves no page behind.
  useEffect(() => {
    setPages((s) => Object.fromEntries(Object.entries(s).filter(([id]) => m.views.includes(id))));
    for (const id of Object.keys(proc.current)) if (!m.views.includes(id)) delete proc.current[id];
  }, [m.views]);

  const backend: FrameBackend = {
    appStatus: (id) => os.appStatus(id),
    appHealth: (id) => os.appHealth(id),
    badge: (id, count, text) => os.frameBadge(id, count, text),
    close,
    minimize,
    pickFolder: (id, title) => os.framePickFolder(id, title),
    pickFile: (id, title) => os.framePickFile(id, title),
    openExternal: (id, url) => os.frameOpenExternal(id, url),
    loginOpen: (id, url) => os.frameLoginOpen(id, url),
    loginClose: (id) => os.frameLoginClose(id),
  };
  const backendRef = useRef(backend);
  backendRef.current = backend;
  const pagesRef = useRef(pages);
  pagesRef.current = pages;
  const mRef = useRef(m);
  mRef.current = m;
  const notifyRef = useRef(notify);
  notifyRef.current = notify;

  // AND-3: the active view is shown in its slot (opened there the first time); a closed view ends its slot's process.
  const shownSlot = useRef<string | null>(null);
  const slotViews = useRef<string[]>([]);
  useEffect(() => {
    if (!slots) return;
    for (const id of slotViews.current) {
      if (m.views.includes(id)) continue;
      send(slots, { op: "close", id });
      slotWin.current.drop(id);
    }
    slotViews.current = [...m.views];
    const id = m.active;
    if (!id) shownSlot.current = null;
    const page = id ? pages[id] : undefined;
    if (id && page && shownSlot.current !== id) {
      shownSlot.current = id;
      send(slots, openOp(id, page, labelOf(m, id), FRAME_FLAG));
    }
  }, [slots, m, pages]);

  // What the slots say: a page's request (answered by frames.ts, as a frame's), back to the desktop, a slot ended.
  useEffect(() => {
    if (!slots) return;
    slots.onmessage = (e) => {
      const ev = parseSlotEvent(e.data);
      if (!ev) return;
      switch (ev.ev) {
        case "request": {
          const frames: FrameEntry[] = Object.entries(pagesRef.current).map(([id, p]) => ({ id, origin: p.origin, window: slotWin.current.token(id) }));
          void answer(frames, { source: slotWin.current.token(ev.id), origin: ev.origin, data: ev.data }, backendRef.current).then((r) => {
            const to = r && slotWin.current.idOf(r.to);
            if (r && to) send(slots, { op: "reply", id: to, message: r.message });
          });
          break;
        }
        case "left":
          if (mRef.current.active === ev.id) minimize(ev.id);
          shownSlot.current = null;
          break;
        case "ended":
          slotWin.current.drop(ev.id);
          if (shownSlot.current === ev.id) shownSlot.current = null;
          slotViews.current = slotViews.current.filter((v) => v !== ev.id);
          close(ev.id);
          notifyRef.current?.(ev.text);
          break;
        case "error":
          if (shownSlot.current === ev.id) shownSlot.current = null;
          notifyRef.current?.(ev.text);
          break;
      }
    };
    send(slots, { op: "hello" });
    return () => void (slots.onmessage = null);
  }, [slots, close, minimize]);

  useEffect(() => {
    if (slots) return;
    const on = (ev: MessageEvent) => {
      const frames: FrameEntry[] = Object.entries(pagesRef.current).map(([id, p]) => ({ id, origin: p.origin, window: windows.current[id] ?? null }));
      void answer(frames, { source: ev.source, origin: ev.origin, data: ev.data }, backendRef.current).then((r) => {
        if (r) (r.to as Window).postMessage(r.message, r.origin);
      });
    };
    window.addEventListener("message", on);
    return () => window.removeEventListener("message", on);
  }, [slots]);

  // The sign-in window of an app (opened for its page by os_login_window_open) reports to THAT app's frame only.
  useEffect(
    () =>
      os.onFrameLogin((e) => {
        const frames: FrameEntry[] = Object.entries(pagesRef.current).map(([id, p]) => ({ id, origin: p.origin, window: slots ? slotWin.current.token(id) : (windows.current[id] ?? null) }));
        const r = e.closed ? eventFor(frames, e.id, "os_login_closed") : e.url ? eventFor(frames, e.id, "os_login_url", { url: e.url }) : null;
        if (r && slots) send(slots, { op: "reply", id: e.id, message: r.message });
        else if (r) (r.to as Window).postMessage(r.message, r.origin);
      }),
    [os, slots],
  );

  return (
    <div className={`absolute inset-0 z-[5] ${m.active ? "" : "pointer-events-none"}`} data-testid="app-area">
      {m.views.map((id) => {
        const area = areaOf(m, id);
        const active = m.active === id;
        const page = pages[id];
        return (
          <div key={id} className={`absolute inset-0 bg-background ${active ? "" : "hidden"}`} data-view={id}>
            {page && !slots && <Frame id={id} page={page} reload={reloads[id] ?? 0} active={active} onWindow={onWindow} />}
            {page && slots && area.kind !== "status" && !errors[id] && (
              <div className="absolute inset-0 grid place-items-center text-sm text-muted-foreground" data-slot-view={id}>
                <Button size="sm" onClick={() => { shownSlot.current = null; send(slots, openOp(id, page, labelOf(m, id), FRAME_FLAG)); shownSlot.current = id; }}>Show {labelOf(m, id)}</Button>
              </div>
            )}
            {(area.kind === "status" || errors[id]) && (
              <div className="absolute inset-0 z-10 grid place-items-center bg-background" role="status" data-view-status={id}>
                <div className="flex flex-col items-center gap-3 text-sm">
                  <span>{errors[id] ? `The app's page could not be shown: ${errors[id]}` : area.kind === "status" ? area.text : ""}</span>
                  {(errors[id] || (area.kind === "status" && area.retry)) && <Button size="sm" onClick={() => { setErrors(({ [id]: _e, ...rest }) => rest); setTick((t) => t + 1); retry(id); }}>Retry</Button>}
                </div>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

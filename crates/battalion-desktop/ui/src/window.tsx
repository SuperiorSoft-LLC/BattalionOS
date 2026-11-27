// SPDX-License-Identifier: MPL-2.0
// OS-WINDOW-1 (WM-6): the host page of a native window `win-<app>-<n>`: the OS-owned WindowFrame bar (it drags the window, so Windows
// snap works with the decorations off) and ONE frame of the app at its route, on the app's own origin.
import { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import "../kit/styles.css";
import { WindowFrame } from "../kit/blocks/window-frame";
import { winTokens } from "../kit/lib/win-tokens";
import { answer, themeEvents, winEventFor, type FrameEntry } from "./frames";
import { identity, nativeBackend } from "./nativeWindow";

interface Tauri {
  core: { invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> };
  event: { listen(name: string, cb: (e: { payload: unknown }) => void): Promise<() => void> };
  window: { getCurrentWindow(): { startDragging(): Promise<void> } };
}
const tauri = () => (window as unknown as { __TAURI__: Tauri }).__TAURI__;
const me = identity((window as unknown as { __BOS_WIN?: unknown }).__BOS_WIN);
const post = (m: { to: unknown; origin: string; message: unknown }) => (m.to as Window).postMessage(m.message, m.origin);

function NativeWindow() {
  const frame = useRef<HTMLIFrameElement>(null);
  const [maximized, setMaximized] = useState(false);
  const entries = (): FrameEntry[] => (me && frame.current ? [{ id: me.app, origin: me.origin, window: frame.current.contentWindow, wid: me.wid }] : []);

  useEffect(() => {
    if (!me) return;
    document.title = me.title || me.app;
    const be = nativeBackend(me, (cmd, args) => tauri().core.invoke(cmd, args));
    const onMessage = async (ev: MessageEvent) => {
      const r = await answer(entries(), { source: ev.source, origin: ev.origin, data: ev.data }, be);
      if (r) post(r);
    };
    window.addEventListener("message", onMessage);
    const sub = tauri().event.listen("os://win", (e) => {
      const p = e.payload as { wid?: string; change?: string; state?: unknown };
      if (p.wid === me.wid && p.change === "state") setMaximized(p.state === "maximized");
      const out = winEventFor(entries(), e.payload);
      if (out) post(out);
    });
    return () => {
      window.removeEventListener("message", onMessage);
      void sub.then((un) => un());
    };
  }, []);

  if (!me) return null;
  const call = (cmd: string) => void tauri().core.invoke(cmd, { wid: me.wid });
  const onLoad = () => {
    const dark = window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? true;
    for (const t of themeEvents(entries(), winTokens(dark))) post(t);
  };
  return (
    <WindowFrame
      title={me.title || me.app}
      maximized={maximized}
      onMinimize={() => call("os_win_minimize")}
      onMaximize={() => call(maximized ? "os_win_restore" : "os_win_maximize")}
      onClose={() => call("os_win_close")}
      onDragStart={() => void tauri().window.getCurrentWindow().startDragging()}
    >
      <iframe ref={frame} title={me.title || me.app} src={me.src} onLoad={onLoad} style={{ flex: 1, border: 0, width: "100%" }} />
    </WindowFrame>
  );
}

createRoot(document.getElementById("root")!).render(<NativeWindow />);

// SPDX-License-Identifier: MPL-2.0
// OS-WINDOW-1 (WM-6): the pure half of a native window's host page (window.html). The window was made for one app and one wid
// (`window.__BOS_WIN`, set by the host before the page runs); its frame speaks to the OS through the same bridge as the frames in
// `main` (frames.ts `answer`), and the window calls go to the host as `os_win_*` from this window (the host names the app from the
// label `win-<app>-<n>`, never from the message).

import { WIN_COMMANDS, type FrameBackend, type WinCommand } from "./frames";

/** What the host put in the page (`native::host_script`). */
export interface NativeIdentity { app: string; wid: string; title: string; origin: string; src: string }

/** The identity, or null when the page was not opened by the host as a native window (then it shows nothing). */
export function identity(v: unknown): NativeIdentity | null {
  const o = v as Record<string, unknown> | null;
  if (!o || typeof o !== "object") return null;
  if (!["app", "wid", "title", "origin", "src"].every((k) => typeof o[k] === "string")) return null;
  const { app, wid, origin, src } = o as unknown as NativeIdentity;
  const n = wid.startsWith(`w-${app}-`) ? wid.slice(app.length + 3) : "";
  if (!/^[0-9]+$/.test(n) || !src.startsWith(`${origin}/`)) return null;
  return o as unknown as NativeIdentity;
}

type Invoke = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

const notHere = (cmd: string) => () => Promise.reject(new Error(`'${cmd}' is not available in a native window`));

/** The bridge backend of the frame in native window `me`. Closing or minimizing the content acts on this window. */
export function nativeBackend(me: NativeIdentity, invoke: Invoke): FrameBackend {
  return {
    // POPOUT-1: the frame finds its daemon like a frame in `main` (its own app only; the host checks the label).
    appStatus: () => invoke("os_app_status", { id: me.app }),
    appHealth: () => invoke("os_app_health", { id: me.app }),
    badge: async (_id, count, text) => {
      await invoke("os_win_set", { wid: me.wid, badge: count === undefined && text === undefined ? null : { count, text } });
    },
    close: () => void invoke("os_win_close", { wid: me.wid }),
    minimize: () => void invoke("os_win_minimize", { wid: me.wid }),
    pickFolder: notHere("os_pick_folder"),
    pickFile: notHere("os_pick_file"),
    openExternal: notHere("os_open_external"),
    loginOpen: notHere("os_login_window_open"),
    loginClose: notHere("os_login_window_close"),
    notify: notHere("os_notify"),
    scanCode: notHere("os_scan_code"),
    win: (_id, cmd: WinCommand, args) => ((WIN_COMMANDS as readonly string[]).includes(cmd) ? invoke(cmd, args) : Promise.reject(new Error(`'${cmd}' is not a window call`))),
  };
}

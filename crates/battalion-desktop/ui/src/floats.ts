// SPDX-License-Identifier: MPL-2.0
// WM-8 (OS-WINDOW-1 §2.2, owner 11-28): the `float` window kind. The OS shell draws an app's float with WindowFrame over the work area
// of the OS window (above the app views, never under the taskbar), holding a frame of the app at its route. Pure: no DOM, so the
// geometry, the taskbar listing and the frame's allow list are tested without a window.

import { clampBox, snapBox, type Box, type Snap, type Surface } from "../kit/lib/geometry";
import { answer, type FrameBackend, type FrameEntry, type FrameEvent, type FrameReply } from "./frames";
import type { Model, Win } from "./store";

/** The query a float's frame gets besides `bos-app`: the wid it is the content of. */
export const FLOAT_WID_FLAG = "bos-wid";

/** What the page in a float may call: the window calls, its window info and its own app's status and health. Nothing wider. */
export const FLOAT_COMMANDS = ["os_window_info", "os_app_status", "os_app_health", "os_win_open", "os_win_close", "os_win_minimize", "os_win_restore", "os_win_maximize", "os_win_focus", "os_win_snap", "os_win_set", "os_win_list"] as const;

const snapOf = (state: unknown): Snap | null => {
  const s = state as { snap?: unknown } | null;
  return s && typeof s === "object" && typeof s.snap === "string" ? (s.snap as Snap) : null;
};

/** Every float of every app, bottom to top (the OS z-order). */
export function floatsOf(m: Model): Win[] {
  return Object.values(m.wins ?? {})
    .flat()
    .filter((w) => w.kind === "float")
    .sort((a, b) => (a.z ?? 0) - (b.z ?? 0));
}

/** Minimized = hidden but kept alive (its frame stays loaded), listed under its app's taskbar button. */
export const floatShown = (w: Win): boolean => w.state !== "minimized";

/** The box a float takes in work area `work` (the desktop area above the taskbar): maximized fills the work area and nothing more,
 *  a snap takes that half or quarter of it, otherwise its own bounds kept on the work area. */
export function floatBox(w: Win, work: Surface): Box {
  if (w.state === "maximized") return { x: work.x ?? 0, y: work.y ?? 0, w: work.w, h: work.h };
  const zone = snapOf(w.state);
  if (zone) return snapBox(zone, work);
  return clampBox(w.bounds ?? { x: 40, y: 40, w: 480, h: 360 }, work);
}

/** The url a float's frame loads: the route on the app's own origin, with the shell's flags. */
export function floatUrl(origin: string, route: string | undefined, app: string, wid: string): string {
  const r = route && route.startsWith("/") && !route.startsWith("//") ? route : "/";
  return `${origin}${r}${r.includes("?") ? "&" : "?"}bos-app=${encodeURIComponent(app)}&${FLOAT_WID_FLAG}=${encodeURIComponent(wid)}`;
}

/** The answer to a message from a float's frame: as frames.ts answers any frame (the app is the frame's, the wid the float's), but a
 *  command outside FLOAT_COMMANDS is refused by name. */
export function answerFloat(frames: readonly FrameEntry[], ev: FrameEvent, be: FrameBackend): Promise<FrameReply | null> {
  const d = ev.data as { cmd?: unknown } | null;
  const cmd = d && typeof d === "object" && typeof d.cmd === "string" ? d.cmd : null;
  if (cmd === null || (FLOAT_COMMANDS as readonly string[]).includes(cmd)) return answer(frames, ev, be);
  // Not served here: answered as a refusal through the same identity checks (a window the shell did not make still gets nothing).
  const refuse: FrameBackend = { ...be, refusal: async () => `'${cmd}' is not available in a float window` };
  return answer(frames, { ...ev, data: { ...(d as object), cmd: `${cmd}#float` } }, refuse);
}

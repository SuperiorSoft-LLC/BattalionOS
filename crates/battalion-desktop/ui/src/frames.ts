// SPDX-License-Identifier: MPL-2.0
// The bridge between an app's page (in a frame the shell made) and the OS (docs/commands.md, "The page in a frame"). Pure: no DOM, so
// identity and the allow list are tested without a window.
//
// The page sends `{bos: 1, id: <request id>, cmd, args?}` to its parent. The shell never believes the message about WHO sent it:
// the app is the frame whose window is `event.source`, and the message must come from that frame's own origin. A message from a
// window the shell did not make gets no answer at all. Only today's app commands are served; the rest are refused.

/** The app commands a page may call from a frame (the `app-*` capability of docs/commands.md), same names and results. */
export const FRAME_COMMANDS = ["os_window_info", "os_app_status", "os_app_health", "os_close_app_window", "os_badge", "os_show_desktop", "os_pick_folder", "os_pick_file", "os_open_external", "os_login_window_open", "os_login_window_close", "os_window_minimize", "os_window_toggle_maximize", "os_window_close", "os_window_start_drag", "os_window_bar"] as const;
/** BOS-27: the calls on the OS window itself. Only the app view that is on screen may use them. */
export const WINDOW_COMMANDS: readonly string[] = ["os_window_minimize", "os_window_toggle_maximize", "os_window_close", "os_window_start_drag", "os_window_bar"];
export type FrameCommand = (typeof FRAME_COMMANDS)[number];

/** A frame the shell made: the app it shows, the origin its page has, and its window (null once the frame is gone). */
export interface FrameEntry { id: string; origin: string; window: unknown; /** BOS-27: this app's view is the one showing in the OS window (active, not minimized). */ onScreen?: boolean }

/** What the shell does for the app of a frame. The app id is always the frame's. */
export interface FrameBackend {
  appStatus(id: string): Promise<unknown>;
  appHealth(id: string): Promise<unknown>;
  badge(id: string, count: number | undefined, text: string | undefined): Promise<void>;
  /** Close = the view ends; the app and its agents go on. */
  close(id: string): void;
  /** Minimize = back to the desktop; the view keeps its state. */
  minimize(id: string): void;
  /** BOS-25: the native calls. The host checks the manifest permission of each (`files.user`, `network`, `login.window`). */
  pickFolder(id: string, title: string | undefined): Promise<string | null>;
  pickFile(id: string, title: string | undefined): Promise<string | null>;
  openExternal(id: string, url: string): Promise<void>;
  loginOpen(id: string, url: string): Promise<void>;
  loginClose(id: string): Promise<void>;
  /** BOS-27: the OS window's own buttons and drag, for the page of the app on screen. Close hides the window, as the Windows button did. */
  windowMinimize(): Promise<void>;
  windowToggleMaximize(): Promise<void>;
  windowClose(): Promise<void>;
  windowStartDrag(): Promise<void>;
  /** BOS-27: the page says its own title bar is there (`on`) or gone. The shell then draws no bar of its own over it, if the manifest declares one. */
  windowBar(id: string, on: boolean): void;
}

export interface FrameEvent { source: unknown; origin: string; data: unknown }
export type FrameReply = { to: unknown; origin: string; message: { bos: 1; id: string; ok: true; result: unknown } | { bos: 1; id: string; ok: false; error: string } };

const REQUEST_ID = /^[A-Za-z0-9_.:-]{1,64}$/;
const text = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));

/** The answer to one message, or null when the message gets nothing (a window the shell did not make, a frame that left its origin, not a request). */
export async function answer(frames: readonly FrameEntry[], ev: FrameEvent, be: FrameBackend): Promise<FrameReply | null> {
  const frame = ev.source == null ? undefined : frames.find((f) => f.window != null && f.window === ev.source);
  if (!frame || ev.origin !== frame.origin) return null;
  const d = ev.data as { bos?: unknown; id?: unknown; cmd?: unknown; args?: unknown } | null;
  if (!d || typeof d !== "object" || d.bos !== 1 || typeof d.id !== "string" || !REQUEST_ID.test(d.id) || typeof d.cmd !== "string") return null;
  const reply = (r: { ok: true; result: unknown } | { ok: false; error: string }): FrameReply => ({ to: frame.window, origin: frame.origin, message: { bos: 1, id: d.id as string, ...r } });
  if (!(FRAME_COMMANDS as readonly string[]).includes(d.cmd)) return reply({ ok: false, error: `'${d.cmd}' is not available to an app's page` });
  if (WINDOW_COMMANDS.includes(d.cmd) && !frame.onScreen) return reply({ ok: false, error: `'${d.cmd}' is only for the app that is on screen` });
  const args = d.args && typeof d.args === "object" ? (d.args as Record<string, unknown>) : {};
  // Today's app commands name no app or only their own; a page can never act for another one.
  if (args.id !== undefined && args.id !== frame.id) return reply({ ok: false, error: `app '${frame.id}' may not act on app '${String(args.id)}'` });
  try {
    switch (d.cmd as FrameCommand) {
      case "os_window_info":
        return reply({ ok: true, result: { label: `app-${frame.id}`, app: frame.id } });
      case "os_app_status":
        return reply({ ok: true, result: await be.appStatus(frame.id) });
      case "os_app_health":
        return reply({ ok: true, result: await be.appHealth(frame.id) });
      case "os_badge": {
        const count = typeof args.count === "number" ? args.count : undefined;
        const t = typeof args.text === "string" ? args.text : undefined;
        await be.badge(frame.id, count, t);
        return reply({ ok: true, result: null });
      }
      case "os_close_app_window":
        be.close(frame.id);
        return reply({ ok: true, result: null });
      case "os_show_desktop":
        be.minimize(frame.id);
        return reply({ ok: true, result: null });
      case "os_pick_folder":
        return reply({ ok: true, result: await be.pickFolder(frame.id, typeof args.title === "string" ? args.title : undefined) });
      case "os_pick_file":
        return reply({ ok: true, result: await be.pickFile(frame.id, typeof args.title === "string" ? args.title : undefined) });
      case "os_open_external":
        if (typeof args.url !== "string") return reply({ ok: false, error: "os_open_external needs a url" });
        await be.openExternal(frame.id, args.url);
        return reply({ ok: true, result: null });
      case "os_login_window_open":
        if (typeof args.url !== "string") return reply({ ok: false, error: "os_login_window_open needs a url" });
        await be.loginOpen(frame.id, args.url);
        return reply({ ok: true, result: null });
      case "os_login_window_close":
        await be.loginClose(frame.id);
        return reply({ ok: true, result: null });
      case "os_window_minimize":
        await be.windowMinimize();
        return reply({ ok: true, result: null });
      case "os_window_toggle_maximize":
        await be.windowToggleMaximize();
        return reply({ ok: true, result: null });
      case "os_window_close":
        await be.windowClose();
        return reply({ ok: true, result: null });
      case "os_window_start_drag":
        await be.windowStartDrag();
        return reply({ ok: true, result: null });
      case "os_window_bar":
        be.windowBar(frame.id, args.on !== false);
        return reply({ ok: true, result: null });
    }
  } catch (e) {
    return reply({ ok: false, error: text(e) });
  }
}

/** BOS-25: an event for ONE app's frame (the sign-in window of that app reached an address, or ended). Sent to that frame only, at its origin. */
export function eventFor(frames: readonly FrameEntry[], appId: string, event: "os_login_url" | "os_login_closed", data?: { url: string }): { to: unknown; origin: string; message: Record<string, unknown> } | null {
  const frame = frames.find((f) => f.id === appId && f.window != null);
  return frame ? { to: frame.window, origin: frame.origin, message: { bos: 1, event, ...(data ?? {}) } } : null;
}

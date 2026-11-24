// SPDX-License-Identifier: MPL-2.0
// The bridge between an app's page (in a frame the shell made) and the OS (docs/commands.md, "The page in a frame"). Pure: no DOM, so
// identity and the allow list are tested without a window.
//
// The page sends `{bos: 1, id: <request id>, cmd, args?}` to its parent. The shell never believes the message about WHO sent it:
// the app is the frame whose window is `event.source`, and the message must come from that frame's own origin. A message from a
// window the shell did not make gets no answer at all. Only today's app commands are served; the rest are refused.

/** The app commands a page may call from a frame (the `app-*` capability of docs/commands.md), same names and results. */
export const FRAME_COMMANDS = ["os_window_info", "os_app_status", "os_app_health", "os_close_app_window", "os_badge", "os_show_desktop"] as const;
export type FrameCommand = (typeof FRAME_COMMANDS)[number];

/** A frame the shell made: the app it shows, the origin its page has, and its window (null once the frame is gone). */
export interface FrameEntry { id: string; origin: string; window: unknown }

/** What the shell does for the app of a frame. The app id is always the frame's. */
export interface FrameBackend {
  appStatus(id: string): Promise<unknown>;
  appHealth(id: string): Promise<unknown>;
  badge(id: string, count: number | undefined, text: string | undefined): Promise<void>;
  /** Close = the view ends; the app and its agents go on. */
  close(id: string): void;
  /** Minimize = back to the desktop; the view keeps its state. */
  minimize(id: string): void;
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
    }
  } catch (e) {
    return reply({ ok: false, error: text(e) });
  }
}

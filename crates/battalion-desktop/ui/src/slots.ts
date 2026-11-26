// SPDX-License-Identifier: MPL-2.0
// AND-3: app slots on Android. There each app's page runs in its own Android process slot (crates/battalion-android, SlotHub.kt),
// not in a frame of this window. The shell keeps the taskbar, back and close; the host tells it what the slots do over one channel
// (`window.bosSlots`, given only to the shell page). Pure parts here, so they are tested without a phone.

/** The channel the Android host gives the shell page (absent everywhere else: the desktop keeps its frames). */
export interface SlotChannel {
  postMessage(message: string): void;
  onmessage: ((e: { data: string }) => void) | null;
}

export function slotChannel(w: unknown = typeof window === "undefined" ? undefined : window): SlotChannel | null {
  const c = (w as { bosSlots?: SlotChannel } | undefined)?.bosSlots;
  return c && typeof c.postMessage === "function" ? c : null;
}

/** What the host says. `id` is always the app of the slot (the host knows it from the slot it gave, never from a page). */
export type SlotEvent =
  | { ev: "request"; id: string; origin: string; data: unknown }
  | { ev: "left"; id: string }
  | { ev: "opened"; id: string; slot: number; pid: number }
  | { ev: "ended"; id: string; reason: "reused" | "ended"; text: string }
  | { ev: "error"; id: string; text: string }
  | { ev: "slots"; id: ""; slots: { slot: number; app: string | null; pid: number }[] };

const EVENTS = ["request", "left", "opened", "ended", "error", "slots"];

export function parseSlotEvent(text: string): SlotEvent | null {
  try {
    const o = JSON.parse(text) as { ev?: unknown; id?: unknown };
    if (!o || typeof o.ev !== "string" || !EVENTS.includes(o.ev) || typeof o.id !== "string") return null;
    return o as SlotEvent;
  } catch {
    return null;
  }
}

/** What the shell sends. */
export type SlotOp =
  | { op: "hello" }
  | { op: "open"; id: string; url: string; origin: string; name: string }
  | { op: "close"; id: string }
  | { op: "reply"; id: string; message: unknown };

export const send = (c: SlotChannel, op: SlotOp) => c.postMessage(JSON.stringify(op));

/**
 * The stand-in window of each app's slot for frames.ts: a request from a slot carries the slot's token as its source, so the same
 * `answer()` that serves a frame on the desktop serves a slot (the app is the slot's, the origin must be the app's page origin).
 */
export class SlotWindows {
  private tokens = new Map<string, object>();
  token(id: string): object {
    let t = this.tokens.get(id);
    if (!t) this.tokens.set(id, (t = { slot: id }));
    return t;
  }
  idOf(token: unknown): string | null {
    for (const [id, t] of this.tokens) if (t === token) return id;
    return null;
  }
  drop(id: string) {
    this.tokens.delete(id);
  }
}

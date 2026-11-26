// SPDX-License-Identifier: MPL-2.0
// AND-3: app slots on Android. There each app's page runs in its own Android process slot (crates/battalion-android, SlotHub.kt),
// not in a frame of this window. The shell keeps the taskbar, back and close; the host tells it what the slots do over one channel
// (`window.bosSlots`, given only to the shell page). Pure parts here, so they are tested without a phone.

import type { AppPage, PageFiles } from "./os";

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
  | { ev: "slots"; id: ""; slots: { slot: number; app: string | null; pid: number }[]; inUse?: number; max?: number }
  | { ev: "login"; id: string; url?: string; closed?: boolean }
  | { ev: "native"; id: string; req: string; ok: boolean; result?: unknown; error?: string };

const EVENTS = ["request", "left", "opened", "ended", "error", "slots", "native", "login"];

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
  | { op: "open"; id: string; url: string; origin: string; name: string; files?: PageFiles }
  | { op: "close"; id: string }
  | { op: "reply"; id: string; message: unknown }
  | { op: "native"; id: string; req: string; call: string; args: unknown }
  | { op: "config"; inUse: number };

/** AND-6: the "Apps open at once" setting the host reports (`inUse` of `max` slots), or null when it says nothing usable. */
export function slotLimit(ev: { inUse?: unknown; max?: unknown }): { inUse: number; max: number } | null {
  const max = typeof ev.max === "number" && Number.isInteger(ev.max) && ev.max >= 1 ? ev.max : null;
  if (max === null || typeof ev.inUse !== "number" || !Number.isInteger(ev.inUse)) return null;
  return { inUse: Math.min(max, Math.max(1, ev.inUse)), max };
}

/** The open op of app `id`: its page url marked as a frame of `id` (`flag`), its origin, and (AND-3b) the files the slot serves. */
export function openOp(id: string, page: AppPage, name: string, flag: string): SlotOp {
  const url = `${page.url}${page.url.includes("?") ? "&" : "?"}${flag}=${encodeURIComponent(id)}`;
  return page.files ? { op: "open", id, url, origin: page.origin, name, files: page.files } : { op: "open", id, url, origin: page.origin, name };
}

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

/**
 * AND-4: native calls run by an app's own slot (document picker, intent, notification, code scanner). The host has already checked
 * the app's permission (`os_frame_phone_call`); the slot answers with a `native` event carrying the same `req`. One promise per call.
 */
export class NativeCalls {
  private next = 0;
  private pending = new Map<string, { id: string; done: (r: { ok: boolean; result?: unknown; error?: string }) => void }>();
  call(c: SlotChannel, id: string, checked: { call: string; args: unknown }): Promise<unknown> {
    const req = `n${++this.next}`;
    return new Promise((resolve, reject) => {
      this.pending.set(req, { id, done: (r) => (r.ok ? resolve(r.result ?? null) : reject(new Error(r.error || "the call failed"))) });
      send(c, { op: "native", id, req, call: checked.call, args: checked.args });
    });
  }
  /** The slot's answer; only the slot of the app that asked can settle its call. */
  settle(ev: { id: string; req: string; ok: boolean; result?: unknown; error?: string }): boolean {
    const p = this.pending.get(ev.req);
    if (!p || p.id !== ev.id) return false;
    this.pending.delete(ev.req);
    p.done(ev);
    return true;
  }
  /** The app's slot ended: its open calls fail. */
  drop(id: string, why = "the app was closed") {
    for (const [req, p] of [...this.pending]) if (p.id === id) (this.pending.delete(req), p.done({ ok: false, error: why }));
  }
}

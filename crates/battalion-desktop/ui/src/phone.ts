// SPDX-License-Identifier: MPL-2.0
// AND-UI-1: the same shell on a phone. The host says which platform it runs on (os_window_info `platform`, Rust's
// std::env::consts::OS); the user agent is only a fallback when the host is too old to say. A phone layout has no window buttons,
// opens with one tap, shows apps full screen with the taskbar as the switcher, and the back gesture goes back to the desktop.
import { useEffect, useRef } from "react";

export type Layout = "desktop" | "phone";

const PHONE_PLATFORMS = ["android", "ios"];

/** The layout for a host platform; with no platform from the host, the user agent decides (Android or iPhone is a phone). */
export function layoutFor(platform: string | null | undefined, userAgent = ""): Layout {
  if (platform) return PHONE_PLATFORMS.includes(platform.toLowerCase()) ? "phone" : "desktop";
  return /Android|iPhone/i.test(userAgent) ? "phone" : "desktop";
}

/** What the back gesture does now: close the Start menu first, then the Armory, else leave the app for the desktop, else nothing (Android's own back). */
export type BackStep = "close-start" | "close-armory" | "to-desktop" | null;
export function backStep(armory: boolean, active: string | null, start = false): BackStep {
  if (start) return "close-start";
  if (armory) return "close-armory";
  if (active) return "to-desktop";
  return null;
}

interface HistoryLike {
  pushState(data: unknown, unused: string): void;
  back(): void;
}

/**
 * The back gesture on a phone. While something is over the desktop (an app or the Armory) the shell holds one history entry; the
 * WebView's back (Android's back gesture or button) pops it and `onBack` runs. When the shell leaves that state by itself (a tap on
 * the taskbar), the entry is taken off again, so a back at the desktop is Android's own.
 */
export class BackGuard {
  private pushed = false;
  constructor(private readonly h: HistoryLike) {}
  /** Called whenever the shell's state changes; `over` is true while an app, the Armory or the Start menu is over the desktop. */
  sync(over: boolean) {
    if (over && !this.pushed) {
      this.h.pushState({ bos: "back" }, "");
      this.pushed = true;
    } else if (!over && this.pushed) {
      this.pushed = false;
      this.h.back();
    }
  }
  /** A popstate: true when it was the shell's entry (the caller then goes back one step). */
  popped(): boolean {
    if (!this.pushed) return false;
    this.pushed = false;
    return true;
  }
}

/** The React side of BackGuard: on a phone only; a desktop window keeps no history. */
export function useBackGesture(enabled: boolean, over: boolean, onBack: () => void) {
  const guard = useRef<BackGuard | null>(null);
  const back = useRef(onBack);
  back.current = onBack;
  useEffect(() => {
    if (!enabled) return;
    const g = (guard.current ??= new BackGuard(window.history));
    const on = () => g.popped() && back.current();
    window.addEventListener("popstate", on);
    return () => window.removeEventListener("popstate", on);
  }, [enabled]);
  useEffect(() => {
    if (enabled) (guard.current ??= new BackGuard(window.history)).sync(over);
  }, [enabled, over]);
}

// SPDX-License-Identifier: MPL-2.0
// AND-6 (ticket f4f965ad): a tap made before the desktop draws is held and replayed, not lost. Two recorders feed it:
// - the page itself (index.html's inline script, `window.__bosEarlyTaps`) from the first HTML parse, before the bundle runs;
// - on Android the host (MainActivity, channel `bosBoot`), which also sees taps on the native splash and the still blank WebView.
// Once the desktop is drawn the shell takes the taps and opens the icon under the last one. Pure parts here, tested without a phone.

export interface EarlyTap { x: number; y: number; t: number }

/** The boot channel the Android host gives the shell page (absent everywhere else). */
export interface BootChannel {
  postMessage(message: string): void;
  onmessage: ((e: { data: string }) => void) | null;
}

type BootWindow = { __bosEarlyTaps?: unknown; __bosReady?: boolean; bosBoot?: BootChannel };

const isTap = (v: unknown): v is EarlyTap => {
  const o = v as EarlyTap;
  return !!o && Number.isFinite(o.x) && Number.isFinite(o.y) && Number.isFinite(o.t);
};

export function parseTaps(v: unknown): EarlyTap[] {
  return Array.isArray(v) ? v.filter(isTap) : [];
}

/** The tap to replay: the newest of all recorded, or null. Older taps are dropped (one tap opens one app). */
export function lastTap(...lists: EarlyTap[][]): EarlyTap | null {
  let best: EarlyTap | null = null;
  for (const l of lists) for (const t of l) if (!best || t.t >= best.t) best = t;
  return best;
}

/** The desktop app under a point, from its `data-app` icon, or null. */
export function appAt(x: number, y: number, doc: Pick<Document, "elementFromPoint"> = document): string | null {
  const el = doc.elementFromPoint(x, y);
  return (el?.closest?.("[data-app]") as HTMLElement | null)?.dataset?.app ?? null;
}

/** The host's taps (empty when there is no boot channel, or it does not answer in `waitMs`). Asking ends the host's recording. */
export function hostTaps(w: BootWindow, waitMs = 1000): Promise<EarlyTap[]> {
  const c = w.bosBoot;
  if (!c || typeof c.postMessage !== "function") return Promise.resolve([]);
  return new Promise((done) => {
    const timer = setTimeout(() => done([]), waitMs);
    c.onmessage = (e) => {
      clearTimeout(timer);
      c.onmessage = null;
      try { done(parseTaps((JSON.parse(e.data) as { taps?: unknown }).taps)); } catch { done([]); }
    };
    c.postMessage(JSON.stringify({ op: "taps" }));
  });
}

/** Ends the page recorder and returns the newest early tap of both recorders. */
export async function takeEarlyTap(w: BootWindow = window as BootWindow): Promise<EarlyTap | null> {
  w.__bosReady = true;
  const page = parseTaps(w.__bosEarlyTaps);
  w.__bosEarlyTaps = [];
  return lastTap(page, await hostTaps(w));
}

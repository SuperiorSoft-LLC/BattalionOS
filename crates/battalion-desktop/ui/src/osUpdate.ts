// SPDX-License-Identifier: MPL-2.0
// BOS-30: BattalionOS's own update, as the Armory shows it. The OS is not an app: one row in Installed, "Update to X", no Open,
// no Remove. The host does the work (os_update_*); this is only what the row needs to say and do.
import type { InstalledApp } from "./os";

export const OS_ROW = "battalionos";

export interface OsUpdateInfo {
  current: string;
  available: { version: string; notes: string } | null;
  installing: boolean;
  /** Running apps that are still doing something (health `busy` above 0). */
  busy: { id: string; name: string; busy: number }[];
  running: string[];
  /** BOS-AND-SELFUPDATE-1: what the host adds to the confirm (Android: its own installer asks once more). */
  confirmNote?: string;
}

export type OsPhase = { kind: "idle" } | { kind: "working" } | { kind: "failed"; error: string };

/** The row's one button. Never Open, never Remove: the OS is not an app. After a failure the same button reads "Try again"
 *  (ARMORY-OSROW-1: one action, never Update and Retry side by side). */
export function osRowActions(info: OsUpdateInfo | null, phase: OsPhase): string[] {
  if (phase.kind === "working" || info?.installing) return [];
  if (phase.kind === "failed") return ["Try again"];
  return info?.available ? [`Update to ${info.available.version}`] : [];
}

/** A small mark on the Armory icon when the OS or an app has an update. */
export function armoryHasUpdate(installed: InstalledApp[], info: OsUpdateInfo | null): boolean {
  return !!info?.available || installed.some((a) => !!a.available);
}

/** The confirm, in words: the restart, the apps that stop first, and, by name, the apps still busy. */
export function confirmText(info: OsUpdateInfo): string {
  const stop = info.running.length > 0 ? "BattalionOS restarts; every running app stops first and starts again." : "BattalionOS restarts.";
  const note = info.confirmNote ? ` ${info.confirmNote}` : "";
  if (info.busy.length === 0) return stop + note;
  const names = info.busy.map((b) => b.name).join(", ");
  return `${stop} ${names} ${info.busy.length === 1 ? "is" : "are"} finishing current work, which would be cut short.${note}`;
}

/** BOS-AND-SELFUPDATE-1: the one-line notice on the desktop when a newer OS is ready, or null (none, installing, or put off). */
export function osReadyText(info: OsUpdateInfo | null, putOff: string | null): string | null {
  if (!info?.available || info.installing || info.available.version === putOff) return null;
  return `BattalionOS ${info.available.version} is ready`;
}

/** What Android's installer step answered (`bosUpdate`, OsUpdate.kt). */
export interface AndroidInstallResult { ok: boolean; error?: string }

/** The channel the Android host gives the shell page for its own update (absent elsewhere). */
export interface UpdateChannel {
  postMessage(message: string): void;
  onmessage: ((e: { data: string }) => void) | null;
}

export function updateChannel(w: unknown = typeof window === "undefined" ? undefined : window): UpdateChannel | null {
  const c = (w as { bosUpdate?: UpdateChannel } | undefined)?.bosUpdate;
  return c && typeof c.postMessage === "function" ? c : null;
}

/** Hand the checked APK to Android's installer and wait for its answer. Success ends this process (Android replaces the app), so a
 *  return is a failure, or a confirm Android is still showing ("pending"). */
export function androidInstall(c: UpdateChannel, apk: string, sha256: string): Promise<AndroidInstallResult> {
  return new Promise((resolve) => {
    c.onmessage = (e) => {
      let r: { ev?: string; ok?: boolean; error?: string; pending?: boolean };
      try { r = JSON.parse(e.data); } catch { return; }
      if (r.ev !== "osInstall" || r.pending) return;
      c.onmessage = null;
      resolve({ ok: r.ok === true, error: r.error });
    };
    c.postMessage(JSON.stringify({ op: "osInstall", apk, sha256 }));
  });
}

export interface OsUpdateApi {
  osUpdateInstall(): Promise<void>;
}

/** Run the update. The host ends this process when it works, so a return means it did not: the text, with Try again. */
export async function runOsUpdate(api: OsUpdateApi, set: (p: OsPhase) => void, errText: (e: unknown) => string): Promise<void> {
  set({ kind: "working" });
  try {
    await api.osUpdateInstall();
  } catch (e) {
    set({ kind: "failed", error: errText(e) });
  }
}

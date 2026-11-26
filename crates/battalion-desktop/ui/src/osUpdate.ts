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
}

export type OsPhase = { kind: "idle" } | { kind: "working" } | { kind: "failed"; error: string };

/** The row's buttons. Never Open, never Remove: the OS is not an app. */
export function osRowActions(info: OsUpdateInfo | null, phase: OsPhase): string[] {
  if (phase.kind === "working" || info?.installing) return [];
  return info?.available ? [`Update to ${info.available.version}`] : [];
}

/** A small mark on the Armory icon when the OS or an app has an update. */
export function armoryHasUpdate(installed: InstalledApp[], info: OsUpdateInfo | null): boolean {
  return !!info?.available || installed.some((a) => !!a.available);
}

/** The confirm, in words: the restart, the apps that stop first, and, by name, the apps still busy. */
export function confirmText(info: OsUpdateInfo): string {
  const stop = info.running.length > 0 ? "BattalionOS restarts; every running app stops first and starts again." : "BattalionOS restarts.";
  if (info.busy.length === 0) return stop;
  const names = info.busy.map((b) => b.name).join(", ");
  return `${stop} ${names} ${info.busy.length === 1 ? "is" : "are"} finishing current work, which would be cut short.`;
}

export interface OsUpdateApi {
  osUpdateInstall(): Promise<void>;
}

/** Run the update. The host ends this process when it works, so a return means it did not: the text, with Retry. */
export async function runOsUpdate(api: OsUpdateApi, set: (p: OsPhase) => void, errText: (e: unknown) => string): Promise<void> {
  set({ kind: "working" });
  try {
    await api.osUpdateInstall();
  } catch (e) {
    set({ kind: "failed", error: errText(e) });
  }
}

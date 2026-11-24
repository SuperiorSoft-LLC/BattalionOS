// SPDX-License-Identifier: MPL-2.0
// The shell's state, pure so every app state, offline and failed install are tested without a window. One owner: the model; the views
// only read it through the selectors below. Nothing here names an app.
import type { AppStateName, Catalog, InstalledApp } from "./os";

export type InstallPhase = { kind: "installing" } | { kind: "failed"; error: string };

export interface Model {
  /** False until the first installed list arrives (the desktop shows a loading state, not the first-run card). */
  loaded: boolean;
  installed: InstalledApp[];
  listError: string | null;
  catalog: Catalog | null;
  /** The catalog could not be read at all (no cache either). */
  catalogError: string | null;
  catalogLoading: boolean;
  installs: Record<string, InstallPhase>;
  firstRunDismissed: boolean;
}

export const initial: Model = { loaded: false, installed: [], listError: null, catalog: null, catalogError: null, catalogLoading: false, installs: {}, firstRunDismissed: false };

export type Action =
  | { type: "installed-loaded"; apps: InstalledApp[] }
  | { type: "installed-failed"; error: string }
  | { type: "catalog-loading" }
  | { type: "catalog-loaded"; catalog: Catalog }
  | { type: "catalog-failed"; error: string }
  | { type: "install-started"; id: string }
  | { type: "install-done"; id: string }
  | { type: "install-failed"; id: string; error: string }
  | { type: "dismiss-first-run" };

export function reduce(m: Model, a: Action): Model {
  switch (a.type) {
    case "installed-loaded":
      return { ...m, loaded: true, installed: a.apps, listError: null };
    case "installed-failed":
      return { ...m, loaded: true, listError: a.error };
    case "catalog-loading":
      return { ...m, catalogLoading: true };
    case "catalog-loaded":
      return { ...m, catalog: a.catalog, catalogError: null, catalogLoading: false };
    case "catalog-failed":
      return { ...m, catalogError: a.error, catalogLoading: false };
    case "install-started":
      return { ...m, installs: { ...m.installs, [a.id]: { kind: "installing" } } };
    case "install-done": {
      const { [a.id]: _gone, ...rest } = m.installs;
      return { ...m, installs: rest };
    }
    case "install-failed":
      return { ...m, installs: { ...m.installs, [a.id]: { kind: "failed", error: a.error } } };
    case "dismiss-first-run":
      return { ...m, firstRunDismissed: true };
  }
}

/** What one app is, for the Armory and the desktop. An install of this session (installing, failed) wins over the stored state. */
export type AppView =
  | { kind: "available" }
  | { kind: "incompatible" }
  | { kind: "installing"; stage: string }
  | { kind: "failed"; error: string }
  | { kind: "installed"; running: boolean; version: string };

const STAGE: Partial<Record<AppStateName, string>> = { downloading: "Downloading", verifying: "Verifying", initialising: "Setting up", updating: "Updating", removing: "Removing" };
const RUNNING: AppStateName[] = ["running", "starting", "stopping"];

export function appView(m: Model, id: string): AppView {
  const phase = m.installs[id];
  if (phase?.kind === "failed") return { kind: "failed", error: phase.error };
  const inst = m.installed.find((a) => a.id === id);
  if (inst?.state === "failed") return { kind: "failed", error: "The app failed. Install it again to repair it." };
  const stage = inst ? STAGE[inst.state] : undefined;
  if (stage) return { kind: "installing", stage };
  if (phase?.kind === "installing") return { kind: "installing", stage: "Installing" };
  if (inst && inst.state !== "absent") return { kind: "installed", running: RUNNING.includes(inst.state), version: inst.version };
  const entry = m.catalog?.apps.find((c) => c.id === id);
  if (entry && !entry.compatible) return { kind: "incompatible" };
  return { kind: "available" };
}

/** The installed apps that get a desktop icon: any state but absent and being removed. */
export const desktopIds = (m: Model): string[] => m.installed.filter((a) => a.state !== "absent" && a.state !== "removing").map((a) => a.id);

/** Taskbar entries: the apps with a live process (running dot), and a failed one (badge). */
export interface TaskItem { id: string; running: boolean; badge: boolean }
export const taskItems = (m: Model): TaskItem[] =>
  m.installed
    .filter((a) => RUNNING.includes(a.state) || a.state === "failed")
    .map((a) => ({ id: a.id, running: a.state === "running", badge: a.state === "failed" }));

/** Offline means the list is a cache, or nothing could be read. A source error beside a live list is only a warning. */
export interface Offline { offline: boolean; reason: string | null }
export function offline(m: Model): Offline {
  if (m.catalogError) return { offline: true, reason: m.catalogError };
  const c = m.catalog;
  if (!c) return { offline: false, reason: null };
  if (c.fromCache) return { offline: true, reason: c.sourceErrors[0] ?? "Showing the last saved list." };
  if (c.sourceErrors.length) return { offline: false, reason: c.sourceErrors[0]! };
  return { offline: false, reason: null };
}

/** Browse list: apps not installed first. */
export const browseIds = (m: Model): string[] => {
  const ids = (m.catalog?.apps ?? []).map((a) => a.id);
  const isIn = (id: string) => m.installed.some((a) => a.id === id && a.state !== "absent");
  return [...ids.filter((i) => !isIn(i)), ...ids.filter(isIn)];
};

/** The first-run card shows once the list is known and empty, until it is dismissed or an install starts. */
export const showFirstRun = (m: Model): boolean => m.loaded && !m.listError && desktopIds(m).length === 0 && !m.firstRunDismissed && Object.keys(m.installs).length === 0;

/** "battalion-notes" -> "Battalion Notes": the catalog carries ids only, so the label is derived from the id. */
export const appLabel = (id: string): string => id.split("-").filter(Boolean).map((w) => w[0]!.toUpperCase() + w.slice(1)).join(" ");

/** The install flow ("it auto installs then should appear on the desktop"): install, then reload the installed list so the icon appears. */
export interface Flow { install(id: string): Promise<void>; listApps(): Promise<InstalledApp[]> }
export async function runInstall(f: Flow, dispatch: (a: Action) => void, id: string, errText: (e: unknown) => string): Promise<boolean> {
  dispatch({ type: "install-started", id });
  try {
    await f.install(id);
  } catch (e) {
    dispatch({ type: "install-failed", id, error: errText(e) });
    return false;
  }
  try {
    dispatch({ type: "installed-loaded", apps: await f.listApps() });
  } catch (e) {
    dispatch({ type: "installed-failed", error: errText(e) });
  }
  dispatch({ type: "install-done", id });
  return true;
}

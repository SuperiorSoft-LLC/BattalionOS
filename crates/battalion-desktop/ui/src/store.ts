// SPDX-License-Identifier: MPL-2.0
// The shell's state, pure so every app state, offline and failed install are tested without a window. One owner: the model; the views
// only read it through the selectors below. Nothing here names an app.
import type { AppStateName, ArmoryOp, ArmoryProgress, Catalog, InstalledApp } from "./os";

export type InstallPhase = { kind: "installing" } | { kind: "failed"; error: string };
/** An update, rollback or remove of this session: running, or failed with the text the Armory gave (Retry runs it again). */
export type OpPhase = { kind: "working"; op: ArmoryOp } | { kind: "failed"; op: ArmoryOp; error: string };

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
  ops: Record<string, OpPhase>;
  /** The last progress report per app while its action runs. */
  progress: Record<string, ArmoryProgress>;
  firstRunDismissed: boolean;
  /** The apps shown inside the window, in the order they were opened; each keeps its frame (and its state) until it is closed. */
  views: string[];
  /** The view that fills the app area, or null (the desktop). A minimized view is in `views` but not active. */
  active: string | null;
}

export const initial: Model = { loaded: false, installed: [], listError: null, catalog: null, catalogError: null, catalogLoading: false, installs: {}, ops: {}, progress: {}, firstRunDismissed: false, views: [], active: null };

export type Action =
  | { type: "installed-loaded"; apps: InstalledApp[] }
  | { type: "installed-failed"; error: string }
  | { type: "catalog-loading" }
  | { type: "catalog-loaded"; catalog: Catalog }
  | { type: "catalog-failed"; error: string }
  | { type: "install-started"; id: string }
  | { type: "install-done"; id: string }
  | { type: "install-failed"; id: string; error: string }
  | { type: "op-started"; id: string; op: ArmoryOp }
  | { type: "op-done"; id: string }
  | { type: "op-failed"; id: string; op: ArmoryOp; error: string }
  | { type: "op-dismissed"; id: string }
  | { type: "progress"; p: ArmoryProgress }
  | { type: "dismiss-first-run" }
  | { type: "view-open"; id: string }
  | { type: "view-minimize"; id: string }
  | { type: "view-close"; id: string };

const without = <T,>(r: Record<string, T>, id: string): Record<string, T> => {
  const { [id]: _gone, ...rest } = r;
  return rest;
};

export function reduce(m: Model, a: Action): Model {
  switch (a.type) {
    case "installed-loaded": {
      // An app that is gone (removed) has no view: its frame ends with it.
      const gone = (id: string) => !a.apps.some((x) => x.id === id && x.state !== "absent" && x.state !== "removing");
      const views = m.views.filter((id) => !gone(id));
      return { ...m, loaded: true, installed: a.apps, listError: null, views, active: m.active && views.includes(m.active) ? m.active : null };
    }
    case "installed-failed":
      return { ...m, loaded: true, listError: a.error };
    case "catalog-loading":
      return { ...m, catalogLoading: true };
    case "catalog-loaded":
      return { ...m, catalog: a.catalog, catalogError: null, catalogLoading: false };
    case "catalog-failed":
      return { ...m, catalogError: a.error, catalogLoading: false };
    case "install-started":
      return { ...m, installs: { ...m.installs, [a.id]: { kind: "installing" } }, progress: without(m.progress, a.id) };
    case "install-done":
      return { ...m, installs: without(m.installs, a.id), progress: without(m.progress, a.id) };
    case "install-failed":
      return { ...m, installs: { ...m.installs, [a.id]: { kind: "failed", error: a.error } }, progress: without(m.progress, a.id) };
    case "op-started":
      return { ...m, ops: { ...m.ops, [a.id]: { kind: "working", op: a.op } }, progress: without(m.progress, a.id) };
    case "op-done":
    case "op-dismissed":
      return { ...m, ops: without(m.ops, a.id), progress: without(m.progress, a.id) };
    case "op-failed":
      return { ...m, ops: { ...m.ops, [a.id]: { kind: "failed", op: a.op, error: a.error } }, progress: without(m.progress, a.id) };
    case "progress":
      return { ...m, progress: { ...m.progress, [a.p.id]: a.p } };
    case "dismiss-first-run":
      return { ...m, firstRunDismissed: true };
    case "view-open":
      return { ...m, views: m.views.includes(a.id) ? m.views : [...m.views, a.id], active: a.id };
    case "view-minimize":
      return { ...m, active: m.active === a.id ? null : m.active };
    case "view-close":
      return { ...m, views: m.views.filter((id) => id !== a.id), active: m.active === a.id ? null : m.active };
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
/** The journal's step names, as the Armory reports them, in words. */
const STEP: Record<string, string> = { download: "Downloading", verify: "Verifying", unpack: "Unpacking", place: "Placing files", init: "Setting up", stop: "Stopping", switch: "Switching version", start: "Starting", done: "Done", mark: "Removing", delete: "Removing files", forget: "Finishing" };
const mb = (n: number) => (n / 1048576).toFixed(n < 10485760 ? 1 : 0);

/** What the progress bar and its caption show: the stage in words and the percent (null = no measure, a moving bar). */
export function progressOf(m: Model, id: string): { text: string; percent: number | null } | null {
  const p = m.progress[id];
  if (!p) return null;
  const text = STEP[p.stage] ?? p.stage;
  const known = p.percent != null;
  return { text: known && p.total ? `${text}: ${mb(p.bytes ?? 0)} of ${mb(p.total)} MB (${p.percent}%)` : text, percent: known ? p.percent! : null };
}
const RUNNING: AppStateName[] = ["running", "starting", "stopping"];

export function appView(m: Model, id: string): AppView {
  const phase = m.installs[id];
  if (phase?.kind === "failed") return { kind: "failed", error: phase.error };
  const inst = m.installed.find((a) => a.id === id);
  if (inst?.state === "failed") return { kind: "failed", error: "The app failed. Install it again to repair it." };
  const stage = inst ? STAGE[inst.state] : undefined;
  const live = m.progress[id] ? STEP[m.progress[id]!.stage] : undefined;
  if (stage) return { kind: "installing", stage: live ?? stage };
  if (phase?.kind === "installing") return { kind: "installing", stage: live ?? "Installing" };
  if (inst && inst.state !== "absent") return { kind: "installed", running: RUNNING.includes(inst.state), version: inst.version };
  const entry = m.catalog?.apps.find((c) => c.id === id);
  if (entry && !entry.compatible) return { kind: "incompatible" };
  return { kind: "available" };
}

/** The installed apps that get a desktop icon: any state but absent and being removed. */
export const desktopIds = (m: Model): string[] => m.installed.filter((a) => a.state !== "absent" && a.state !== "removing").map((a) => a.id);

/** Taskbar entries: the apps with a live process (running dot), and a failed one (badge). */
export interface TaskItem { id: string; running: boolean; badge: boolean; mark: string | null }
export const taskItems = (m: Model): TaskItem[] =>
  m.installed
    .filter((a) => RUNNING.includes(a.state) || a.state === "failed")
    .map((a) => ({ id: a.id, running: a.state === "running", badge: a.state === "failed", mark: a.badge ?? null }));

/** A taskbar button: a live or failed app (`taskItems`), and any app with a view, even one that stopped. `active` = it fills the app area. */
export interface BarItem extends TaskItem { open: boolean; active: boolean }
export const barItems = (m: Model): BarItem[] => {
  const live = taskItems(m);
  const extra = m.views.filter((id) => !live.some((t) => t.id === id)).map((id) => ({ id, running: false, badge: false, mark: m.installed.find((a) => a.id === id)?.badge ?? null }));
  return [...live, ...extra].map((t) => ({ ...t, open: m.views.includes(t.id), active: m.active === t.id }));
};

/** What the app area shows for a view: the page, or the app's state (with Retry where starting it again helps). */
export type Area = { kind: "frame" } | { kind: "status"; text: string; retry: boolean };
export function areaOf(m: Model, id: string): Area {
  const a = m.installed.find((x) => x.id === id);
  switch (a?.state) {
    case "running":
      return { kind: "frame" };
    case "starting":
    case "initialising":
      return { kind: "status", text: "Starting…", retry: false };
    case "stopping":
      return { kind: "status", text: "Stopping…", retry: false };
    case "updating":
      return { kind: "status", text: "Updating…", retry: false };
    case "failed":
      return { kind: "status", text: "The app stopped working.", retry: true };
    case undefined:
    case "absent":
    case "removing":
      return { kind: "status", text: "The app is not installed.", retry: false };
    default:
      return { kind: "status", text: "The app is stopped.", retry: true };
  }
}

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

/** What the Armory shows for an app: its name from the installed manifest, else the catalog, else a label made of the id. */
export const labelOf = (m: Model, id: string): string => m.installed.find((a) => a.id === id)?.name || m.catalog?.apps.find((a) => a.id === id)?.name || appLabel(id);
/** Its icon (a data URL or an https URL), or null for the generic glyph. */
export const iconOf = (m: Model, id: string): string | null => m.installed.find((a) => a.id === id)?.icon || m.catalog?.apps.find((a) => a.id === id)?.icon || null;
/** The newer version on offer for an installed app, or null. */
export const availableOf = (m: Model, id: string): string | null => m.installed.find((a) => a.id === id)?.available ?? null;
/** The version a rollback goes back to, or null. */
export const previousOf = (m: Model, id: string): string | null => m.installed.find((a) => a.id === id)?.previous ?? null;

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

/** Update, rollback or remove: run it, reload the installed list, and keep the error for Retry when it fails. Only this app's phase changes. */
export interface OpFlow { run(id: string, op: ArmoryOp): Promise<void>; listApps(): Promise<InstalledApp[]> }
export async function runOp(f: OpFlow, dispatch: (a: Action) => void, id: string, op: ArmoryOp, errText: (e: unknown) => string): Promise<boolean> {
  dispatch({ type: "op-started", id, op });
  let ok = true;
  try {
    await f.run(id, op);
  } catch (e) {
    ok = false;
    dispatch({ type: "op-failed", id, op, error: errText(e) });
  }
  try {
    dispatch({ type: "installed-loaded", apps: await f.listApps() });
  } catch (e) {
    if (ok) dispatch({ type: "installed-failed", error: errText(e) });
  }
  if (ok) dispatch({ type: "op-done", id });
  return ok;
}

/** BOS-29: the Armory is part of the OS, not an app. Its desktop icon has this fixed id (a colon never occurs in an app id); nothing about it is stored in app.json or the catalog. */
export const ARMORY_ICON = "os:armory";
/** The desktop's icons: the Armory first, then one per installed app. It is always there (no app installed, offline) and is not in the installed list. */
export const iconIds = (m: Model): string[] => [ARMORY_ICON, ...desktopIds(m)];

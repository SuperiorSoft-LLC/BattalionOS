// SPDX-License-Identifier: MPL-2.0
// The shell's state, pure so every app state, offline and failed install are tested without a window. One owner: the model; the views
// only read it through the selectors below. Nothing here names an app.
import type { AppStateName, ArmoryOp, ArmoryProgress, Catalog, InstalledApp, NotReady } from "./os";

export type InstallPhase = { kind: "installing" } | { kind: "failed"; error: string };
/** An update, rollback or remove of this session: running, or failed with the text the Armory gave (Retry runs it again). */
export type OpPhase = { kind: "working"; op: ArmoryOp } | { kind: "failed"; op: ArmoryOp; error: string } | { kind: "not-ready"; op: ArmoryOp; version: string; reason: string; etaMs: number | null };

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
  /** AND-6: apps whose phone slot ended by itself (crash, memory, reuse). Their taskbar button stays, marked ended; a tap opens them fresh. */
  ended: string[];
  /** WM-3: each app's open windows (from the WM, `os_win_list`), by app id. */
  wins?: Record<string, Win[]>;
  /** WM-3: `bos.taskbar.groupWindows` (default true); false = one taskbar button per window. */
  groupWindows?: boolean;
}

/** WM-3: one window row as the WM lists it (the fields the taskbar reads). */
export interface Win { wid: string; app: string; title: string; icon?: string | null; badge?: { count?: number; text?: string } | null; attention?: boolean; state: unknown; z?: number }

export const initial: Model = { loaded: false, installed: [], listError: null, catalog: null, catalogError: null, catalogLoading: false, installs: {}, ops: {}, progress: {}, firstRunDismissed: false, views: [], active: null, ended: [] };

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
  | { type: "op-not-ready"; id: string; version: string; reason: string; etaMs: number | null }
  | { type: "progress"; p: ArmoryProgress }
  | { type: "dismiss-first-run" }
  | { type: "view-open"; id: string }
  | { type: "view-minimize"; id: string }
  | { type: "view-close"; id: string }
  | { type: "view-ended"; id: string }
  | { type: "wins-loaded"; id: string; wins: Win[] }
  | { type: "win-closed"; id: string; wid: string }
  | { type: "group-windows"; on: boolean };

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
    case "op-not-ready":
      return { ...m, ops: { ...m.ops, [a.id]: { kind: "not-ready", op: "update", version: a.version, reason: a.reason, etaMs: a.etaMs } }, progress: without(m.progress, a.id) };
    case "progress":
      return { ...m, progress: { ...m.progress, [a.p.id]: a.p } };
    case "dismiss-first-run":
      return { ...m, firstRunDismissed: true };
    case "view-open":
      return { ...m, views: m.views.includes(a.id) ? m.views : [...m.views, a.id], active: a.id, ended: m.ended.filter((id) => id !== a.id) };
    case "view-minimize":
      return { ...m, active: m.active === a.id ? null : m.active };
    case "view-close":
      return { ...m, views: m.views.filter((id) => id !== a.id), active: m.active === a.id ? null : m.active, ended: m.ended.filter((id) => id !== a.id) };
    case "view-ended":
      return { ...m, views: m.views.filter((id) => id !== a.id), active: m.active === a.id ? null : m.active, ended: m.ended.includes(a.id) ? m.ended : [...m.ended, a.id] };
    case "wins-loaded":
      return { ...m, wins: a.wins.length ? { ...(m.wins ?? {}), [a.id]: a.wins } : without(m.wins ?? {}, a.id) };
    case "win-closed": {
      const left = (m.wins?.[a.id] ?? []).filter((w) => w.wid !== a.wid);
      return { ...m, wins: left.length ? { ...(m.wins ?? {}), [a.id]: left } : without(m.wins ?? {}, a.id) };
    }
    case "group-windows":
      return { ...m, groupWindows: a.on };
  }
}

/** What one app is, for the Armory and the desktop. An install of this session (installing, failed) wins over the stored state. */
export type AppView =
  | { kind: "available" }
  | { kind: "incompatible" }
  | { kind: "unavailable"; reason: string }
  | { kind: "installing"; stage: string }
  | { kind: "failed"; error: string }
  | { kind: "installed"; running: boolean; version: string };

const STAGE: Partial<Record<AppStateName, string>> = { downloading: "Downloading", verifying: "Verifying", initialising: "Setting up", updating: "Updating", removing: "Removing" };
/** The journal's step names, as the Armory reports them, in words. */
const STEP: Record<string, string> = { download: "Downloading", verify: "Verifying", unpack: "Unpacking", place: "Placing files", prepare: "Waiting for the app", init: "Setting up", stop: "Stopping", switch: "Switching version", start: "Starting", done: "Done", mark: "Removing", delete: "Removing files", forget: "Finishing" };
const mb = (n: number) => (n / 1048576).toFixed(n < 10485760 ? 1 : 0);

/** What the progress bar and its caption show: the stage in words and the percent (null = no measure, a moving bar). */
/** UPDATE-LIVE-1: "about 1 min", "about 20 s"; null when the app gave no estimate. */
export function aboutText(ms: number | null | undefined): string | null {
  if (ms == null || !(ms > 0)) return null;
  const s = Math.ceil(ms / 1000);
  return s < 60 ? `about ${s} s` : `about ${Math.round(s / 60)} min`;
}
/** "Agent Hub will update when its agents finish their current step (about 1 min)." */
export const waitText = (label: string, reason: string, etaMs?: number | null): string => {
  const about = aboutText(etaMs);
  return `${label} will update when ${reason}${about ? ` (${about})` : ""}.`;
};

export function progressOf(m: Model, id: string): { text: string; percent: number | null } | null {
  const p = m.progress[id];
  if (!p) return null;
  if (p.stage === "prepare" && p.reason) return { text: waitText(labelOf(m, id), p.reason, p.etaMs), percent: null };
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
  if (entry?.unavailable) return { kind: "unavailable", reason: entry.unavailable };
  return { kind: "available" };
}

/** BOS-32: opening an installed app that is not running starts it. Decided on a model that HAS the app: a window that has just
 *  opened (`--open <id>`, the first start) holds an empty list until the first read, and would otherwise skip the start. */
export const needsStart = (m: Model, id: string): boolean => { const v = appView(m, id); return v.kind === "installed" && !v.running; };

/** The installed apps that get a desktop icon: any state but absent and being removed. */
export const desktopIds = (m: Model): string[] => m.installed.filter((a) => a.state !== "absent" && a.state !== "removing").map((a) => a.id);

/** Taskbar entries: the apps with a live process (running dot), and a failed one (badge). */
export interface TaskItem { id: string; running: boolean; badge: boolean; mark: string | null }
export const taskItems = (m: Model): TaskItem[] =>
  m.installed
    .filter((a) => RUNNING.includes(a.state) || a.state === "failed")
    .map((a) => ({ id: a.id, running: a.state === "running", badge: a.state === "failed", mark: a.badge ?? null }));

/** A taskbar button: a live or failed app (`taskItems`), and any app with a view, even one that stopped. `active` = it fills the app area. */
/** `ended` (AND-6): the app's phone slot ended; the button stays, not running, and a tap opens the app fresh (as Windows shows a closed app). */
/** WM-3: `key` is unique per button; `windows` are the app's open windows (grouped), or the one window of an ungrouped button (`wid`).
 *  `mark` is the app badge (`os_badge`), or, when there is none, the sum of its windows' counts. `attention`: a window asks for it. */
export interface BarWindow { wid: string; title: string; icon: string | null; mark: string | null; attention: boolean; minimized: boolean }
export interface BarItem extends TaskItem { key: string; open: boolean; active: boolean; ended: boolean; windows: BarWindow[]; wid?: string; attention: boolean }

const winMark = (w: Win): string | null => (w.badge?.text ? w.badge.text : w.badge?.count ? String(w.badge.count) : null);
const barWindow = (w: Win): BarWindow => ({ wid: w.wid, title: w.title, icon: w.icon ?? null, mark: winMark(w), attention: w.attention === true, minimized: w.state === "minimized" });
/** The sum of the windows' counts, or null when none has one. */
export const winCountSum = (ws: readonly Win[]): number | null => {
  const counts = ws.map((w) => w.badge?.count ?? 0);
  const sum = counts.reduce((a, b) => a + b, 0);
  return sum > 0 ? sum : null;
};

export const barItems = (m: Model): BarItem[] => {
  const live = taskItems(m);
  const extra = [...m.views, ...(m.ended ?? [])].filter((id, i, all) => all.indexOf(id) === i && !live.some((t) => t.id === id)).map((id) => ({ id, running: false, badge: false, mark: m.installed.find((a) => a.id === id)?.badge ?? null }));
  const group = m.groupWindows !== false;
  return [...live, ...extra].flatMap((t): BarItem[] => {
    const ended = (m.ended ?? []).includes(t.id) && !m.views.includes(t.id);
    const wins = ended ? [] : [...(m.wins?.[t.id] ?? [])].sort((a, b) => (a.z ?? 0) - (b.z ?? 0));
    const sum = winCountSum(wins);
    const base = { ...t, running: ended ? false : t.running, open: m.views.includes(t.id), active: m.active === t.id, ended };
    if (group || wins.length === 0) {
      return [{ ...base, key: t.id, mark: t.mark ?? (sum === null ? null : String(sum)), windows: wins.map(barWindow), attention: wins.some((w) => w.attention === true) }];
    }
    // Ungrouped ("never combine"): one button per window, each with its own badge and attention.
    return wins.map((w) => { const bw = barWindow(w); return { ...base, key: `${t.id}/${w.wid}`, wid: w.wid, mark: bw.mark, windows: [bw], attention: bw.attention }; });
  });
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
/** UPDATE-LIVE-1: the version "Later" was chosen for. */
export const pendingOf = (m: Model, id: string): string | null => m.installed.find((a) => a.id === id)?.pendingUpdate ?? null;

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
export interface OpFlow { run(id: string, op: ArmoryOp): Promise<NotReady | void>; listApps(): Promise<InstalledApp[]> }
export async function runOp(f: OpFlow, dispatch: (a: Action) => void, id: string, op: ArmoryOp, errText: (e: unknown) => string): Promise<boolean> {
  dispatch({ type: "op-started", id, op });
  let ok = true;
  try {
    const r = await f.run(id, op);
    if (r && r.notReady) {
      ok = false;
      dispatch({ type: "op-not-ready", id, version: r.version, reason: r.reason, etaMs: r.etaMs ?? null });
    }
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

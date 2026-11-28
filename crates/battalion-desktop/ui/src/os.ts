// SPDX-License-Identifier: MPL-2.0
// The one place the shell talks to the Rust host: the frozen os_* commands (docs/commands.md) and events. Nothing else invokes.
export type AppStateName = "absent" | "downloading" | "verifying" | "installed" | "initialising" | "stopped" | "starting" | "running" | "stopping" | "updating" | "failed" | "removing";
export interface InstalledApp {
  id: string;
  version: string;
  state: AppStateName;
  channel: string;
  /** From the installed manifest; the shell falls back to a label made of the id. */
  name?: string | null;
  /** A data URL of the manifest icon. */
  icon?: string | null;
  /** The newer version the last feed read offered. */
  available?: string | null;
  /** The version a rollback goes back to. */
  previous?: string | null;
  /** The badge the running app put on itself (`os_badge`), or null. */
  badge?: string | null;
  /** UPDATE-LIVE-1: "Later" was chosen for this version; it applies by itself at the next start or idle moment. */
  pendingUpdate?: string | null;
  /** ARMORY-CHANNEL-SWITCH: the channels the app's catalog entry lists (cached catalog; empty when unknown). */
  channels?: string[];
  /** The version the app's channel offers (cached feed, newer or not); null when the channel has no build yet. */
  channelVersion?: string | null;
}
/** UPDATE-LIVE-1: what `os_update_app` answers when the running app did not get ready in time (the new version stays staged). */
export interface NotReady { notReady: true; version: string; reason: string; etaMs?: number | null }
export interface CatalogEntry { id: string; source: string; compatible: boolean; name?: string | null; publisher?: string | null; summary?: string | null; icon?: string | null; version?: string | null; unavailable?: string | null; /** LOCAL-REPO-1: from a local repo on this machine. */ fromLocalRepo?: boolean; label?: string | null }
/** LOCAL-REPO-1: a private local app repo (`os_local_repos`). `registered` = listed in os.json, managed there. */
export interface LocalRepo { name: string; path: string; registered: boolean }
export interface Catalog { apps: CatalogEntry[]; rejected: unknown[]; sourceErrors: string[]; fromCache: boolean }
/** AND-3b: where the page's files are, when the host serves them itself by request interception (Android; absent on the desktop). */
export interface PageFiles { root: string; pageDir: string; csp: string }
export interface AppPage { id: string; version: string; origin: string; url: string; files?: PageFiles }
import type { FirstRunEvent } from "./firstRunNotice";
import { androidInstall, updateChannel, type OsUpdateInfo } from "./osUpdate";
export type ArmoryOp= "install" | "update" | "rollback" | "remove";
/** `os://armory-progress`: bytes, total and percent only while the stage is `download`. */
export interface ArmoryProgress { id: string; op: ArmoryOp; stage: string; bytes?: number | null; total?: number | null; percent?: number | null; reason?: string | null; etaMs?: number | null }

/** BOS-27: the OS window (no Windows title bar; the shell draws it). Close hides the window; the OS and apps go on. */
export type ResizeEdge = "North" | "South" | "East" | "West" | "NorthEast" | "NorthWest" | "SouthEast" | "SouthWest";
export interface OsWindow {
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  close(): Promise<void>;
  startDrag(): Promise<void>;
  startResize(edge: ResizeEdge): Promise<void>;
  /** Calls [cb] with the maximized state now and at every change; returns the unsubscribe. */
  onMaximized(cb: (maximized: boolean) => void): () => void;
  /** BOS-LAYOUT-1: logical px the maximized window reaches past the monitor's work area (all 0 when not maximized). Absent: none. */
  workAreaInset?(): Promise<Inset>;
}
export interface Inset { top: number; right: number; bottom: number; left: number }

export interface Os {
  window: OsWindow;
  listApps(): Promise<InstalledApp[]>;
  catalog(): Promise<Catalog>;
  /** LOCAL-REPO-1: the local repos and their add / rename / remove. Optional: a host without them shows no Local repos tab. */
  localRepos?(): Promise<LocalRepo[]>;
  addLocalRepo?(name: string): Promise<void>;
  renameLocalRepo?(name: string, newName: string): Promise<void>;
  removeLocalRepo?(name: string): Promise<void>;
  install(id: string, channel?: string): Promise<void>;
  /** Resolves with [NotReady] when the app did not get ready in time; `now` = "Update now". */
  update(id: string, now?: boolean): Promise<NotReady | void>;
  /** "Later": the update applies by itself at the app's next start or idle moment. */
  updateLater(id: string): Promise<void>;
  /** ARMORY-CHANNEL-SWITCH: move an installed app to another listed channel. Installs nothing; an update still never downgrades. */
  setChannel(id: string, channel: string): Promise<void>;
  /** An app was updated (`os://app-updated`); returns the unsubscribe. */
  onAppUpdated(cb: (id: string) => void): () => void;
  remove(id: string): Promise<void>;
  rollback(id: string): Promise<void>;
  startApp(id: string): Promise<void>;
  /** `os_open_app_window`: opens the app inside this window (the host checks it, then emits `os://app-open`, heard by `onOpen`). */
  openAppWindow(id: string): Promise<void>;
  /** The page of an app for its frame: the origin it has and its entry url (`os_app_page`). */
  appPage(id: string): Promise<AppPage>;
  appStatus(id: string): Promise<unknown>;
  appHealth(id: string): Promise<unknown>;
  /** `os_frame_badge`: the badge of the app whose frame asked. */
  frameBadge(id: string, count?: number, text?: string): Promise<void>;
  /** An app is to be shown inside the window; returns the unsubscribe. */
  onOpen(cb: (id: string) => void): () => void;
  /** BOS-25: the native calls for the app of a frame (`os_frame_*`); the host checks the manifest permission. */
  framePickFolder(id: string, title?: string): Promise<string | null>;
  framePickFile(id: string, title?: string): Promise<string | null>;
  frameOpenExternal(id: string, url: string): Promise<void>;
  frameLoginOpen(id: string, url: string): Promise<void>;
  frameLoginClose(id: string): Promise<void>;
  /** BOS-NOTIFY-1: a desktop notice of the app of a frame (`os_frame_notify`; `notifications`, rate-limited per app). */
  frameNotify(id: string, args: { title: string; text?: string; tag?: string; route?: string }): Promise<void>;
  /** BOS-NOTIFY-1: the person clicked a desktop notice of app `id` (`os://frame-notify-click`); returns the unsubscribe. */
  onFrameNotifyClick(cb: (e: { id: string; tag: string | null; route: string | null }) => void): () => void;
  /** FS-API-1: a file call of the app of a frame (`os_frame_fs`); the host checks `files.read` and the path. */
  frameFs(id: string, cmd: string, args: Record<string, unknown>): Promise<unknown>;
  /** FS-API-1: a watched path of app `id` changed (`os://frame-fs-changed`); returns the unsubscribe. */
  onFrameFsChanged(cb: (e: { id: string; watch: string; path: string; change: string }) => void): () => void;
  /** FS-API-1: the OS root folder (`os_fs_info`), shown in Settings. */
  fsInfo(): Promise<{ root: string }>;
  /** AND-4: a page's native call on a phone, checked by the host against the app's manifest: `{call, args}` for its slot. */
  framePhoneCall(id: string, cmd: string, args?: Record<string, unknown>): Promise<{ call: string; args: unknown }>;
  /** WM-2: a window call for the app of a frame (`os_frame_win_<x>` for `os_win_<x>`); the host checks the `window` permission. */
  frameWin(id: string, cmd: string, args: Record<string, unknown>): Promise<unknown>;
  /** WM-8: the OS moved or resized float `wid` of app `id` (`os_shell_win_bounds`). */
  shellWinBounds?(id: string, wid: string, bounds: { x: number; y: number; w: number; h: number }): Promise<unknown>;
  /** WM-8: "Pop out to its own window": float `wid` becomes a native window, same key and route (`os_shell_win_pop_out`). */
  shellWinPopOut?(id: string, wid: string): Promise<unknown>;
  /** WM-2: `os://win` (the payload as the host sent it, `{bos:1, event:"os_win", id, wid, change, ...}`); returns the unsubscribe. */
  onWin(cb: (payload: unknown) => void): () => void;
  /** The sign-in window of an app reached an address (`os://frame-login-url`) or ended (`os://frame-login-closed`); returns the unsubscribe. */
  onFrameLogin(cb: (e: { id: string; url?: string; closed?: boolean }) => void): () => void;
  /** BOS-19: the app a start from outside asked for (`--open <id>`), once; `null` when none (`os_take_open_request`). */
  takeOpenRequest(): Promise<string | null>;
  /** A start from outside asked for an app while this window runs (`os://open-request`); returns the unsubscribe. */
  onOpenRequest(cb: () => void): () => void;
  openLogin(): Promise<void>;
  /** Ends the OS process only; running apps keep running. */
  exit(): Promise<void>;
  /** Calls [cb] on any change the host pushes; returns the unsubscribe. */
  onChange(cb: () => void): () => void;
  /** Install, update, rollback and remove progress; returns the unsubscribe. */
  onProgress(cb: (p: ArmoryProgress) => void): () => void;
  /** BOS-30: what the OS knows about its own update (`os_update_state`; no network). */
  osUpdateState(): Promise<OsUpdateInfo>;
  /** Read the OS feed now (`os_update_check`). A missing feed is not an error: the answer is what was known. */
  osUpdateCheck(): Promise<OsUpdateInfo>;
  /** Download, verify, stop the apps, install, start again (`os_update_install`). Returns only when it did not work. */
  osUpdateInstall(): Promise<void>;
  /** A check of the OS feed finished (`os://os-update`); returns the unsubscribe. */
  onOsUpdate(cb: (i: OsUpdateInfo) => void): () => void;
  /** The apps the first run just installed, once (`os_take_first_run_notice`). */
  takeFirstRunNotice(): Promise<string[]>;
  /** The first-run install of the default apps progressed (`os://first-run`); returns the unsubscribe. */
  onFirstRun(cb: (e: FirstRunEvent) => void): () => void;
}

interface Tauri {
  core: { invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> };
  event: { listen(name: string, cb: (e: { payload: unknown }) => void): Promise<() => void> };
}
const tauri = (): Tauri => {
  const t = (globalThis as { __TAURI__?: Tauri }).__TAURI__;
  if (!t) throw new Error("BattalionOS host not available");
  return t;
};
const call = <T,>(cmd: string, args?: Record<string, unknown>) => tauri().core.invoke(cmd, args) as Promise<T>;

type TauriWindow = { minimize(): Promise<void>; toggleMaximize(): Promise<void>; close(): Promise<void>; startDragging(): Promise<void>; startResizeDragging(d: string): Promise<void>; isMaximized(): Promise<boolean>; onResized(cb: () => void): Promise<() => void> };
const win = () => (tauri() as unknown as { window: { getCurrentWindow(): TauriWindow } }).window.getCurrentWindow();

/** BOS-31: a developer build says so (the window mark), and reports its dev apps' reloads and load failures. Absent in a release build. */
export interface DevAppEvent { id: string; status: "started" | "restarted" | "failed"; ms?: number; error?: string }
export const devMark = (): Promise<boolean> => (call<{ dev?: boolean }>("os_window_info").then((r) => r.dev === true, () => false));
/** AND-UI-1: the platform the host runs on (`android`, `windows`, ...), or null when the host does not say (or is absent). */
/** WM-3: the setting `bos.taskbar.groupWindows` (os.json `{"taskbar": {"groupWindows": false}}`); true unless the host says false. */
export const taskbarGroupWindows = (): Promise<boolean> => call<{ groupWindows?: boolean }>("os_window_info").then((r) => r.groupWindows !== false, () => true);
export const hostPlatform = (): Promise<string | null> => {
  try {
    return call<{ platform?: string }>("os_window_info").then((r) => r.platform ?? null, () => null);
  } catch {
    return Promise.resolve(null);
  }
};
export const onDevApp = (cb: (e: DevAppEvent) => void): (() => void) => {
  const sub = tauri().event.listen("os://dev-app", (e) => cb(e.payload as DevAppEvent));
  return () => void sub.then((off) => off());
};

export const hostOs: Os = {
  window: {
    minimize: () => win().minimize(),
    toggleMaximize: () => win().toggleMaximize(),
    close: () => win().close(),
    startDrag: () => win().startDragging(),
    startResize: (edge) => win().startResizeDragging(edge),
    onMaximized(cb) {
      const w = win();
      const look = () => void w.isMaximized().then(cb, () => {});
      look();
      const sub = w.onResized(look);
      return () => void sub.then((off) => off());
    },
    workAreaInset: () => call<Inset>("os_main_inset"),
  },
  listApps: () => call("os_list_apps"),
  catalog: async () => normalizeCatalog(await call<Catalog>("os_catalog")),
  localRepos: async () => (await call<{ repos: LocalRepo[] }>("os_local_repos")).repos,
  addLocalRepo: async (name) => void (await call("os_local_repo_add", { name })),
  renameLocalRepo: async (name, newName) => void (await call("os_local_repo_rename", { name, newName })),
  removeLocalRepo: async (name) => void (await call("os_local_repo_remove", { name })),
  install: async (id, channel) => void (await call("os_install_app", { id, channel })),
  update: async (id, now) => {
    const r = await call<Record<string, unknown>>("os_update_app", { id, now: now ?? false });
    return r && r.notReady ? (r as unknown as NotReady) : undefined;
  },
  updateLater: async (id) => void (await call("os_update_later", { id })),
  setChannel: async (id, channel) => void (await call("os_set_app_channel", { id, channel })),
  onAppUpdated(cb) {
    const sub = tauri().event.listen("os://app-updated", (e) => cb((e.payload as { id: string }).id));
    return () => void sub.then((off) => off());
  },
  remove: async (id) => void (await call("os_remove_app", { id })),
  rollback: async (id) => void (await call("os_rollback_app", { id })),
  startApp: async (id) => void (await call("os_start_app", { id })),
  openAppWindow: (id) => call("os_open_app_window", { id }),
  appPage: (id) => call("os_app_page", { id }),
  appStatus: (id) => call("os_app_status", { id }),
  appHealth: (id) => call("os_app_health", { id }),
  frameBadge: async (id, count, text) => void (await call("os_frame_badge", { id, count, text })),
  onOpen(cb) {
    const sub = tauri().event.listen("os://app-open", (e) => cb(String(e.payload)));
    return () => void sub.then((off) => off());
  },
  framePickFolder: (id, title) => call("os_frame_pick_folder", { id, title }),
  framePickFile: (id, title) => call("os_frame_pick_file", { id, title }),
  frameOpenExternal: async (id, url) => void (await call("os_frame_open_external", { id, url })),
  frameLoginOpen: async (id, url) => void (await call("os_frame_login_open", { id, url })),
  frameLoginClose: async (id) => void (await call("os_frame_login_close", { id })),
  frameNotify: async (id, args) => void (await call("os_frame_notify", { id, args })),
  onFrameNotifyClick(cb) {
    const sub = tauri().event.listen("os://frame-notify-click", (e) => cb(e.payload as { id: string; tag: string | null; route: string | null }));
    return () => void sub.then((off) => off());
  },
  frameFs: (id, cmd, args) => call("os_frame_fs", { id, cmd, args }),
  onFrameFsChanged(cb) {
    const sub = tauri().event.listen("os://frame-fs-changed", (e) => cb(e.payload as { id: string; watch: string; path: string; change: string }));
    return () => void sub.then((off) => off());
  },
  fsInfo: () => call("os_fs_info"),
  framePhoneCall: (id, cmd, args) => call("os_frame_phone_call", { id, cmd, args: args ?? {} }),
  frameWin: (id, cmd, args) => call(cmd.replace(/^os_win_/, "os_frame_win_"), { id, args }),
  shellWinBounds: (id, wid, bounds) => call("os_shell_win_bounds", { id, wid, bounds }),
  shellWinPopOut: (id, wid) => call("os_shell_win_pop_out", { id, wid }),
  onWin(cb) {
    const sub = tauri().event.listen("os://win", (e) => cb(e.payload));
    return () => void sub.then((off) => off());
  },
  onFrameLogin(cb) {
    const a = tauri().event.listen("os://frame-login-url", (e) => cb(e.payload as { id: string; url: string }));
    const b = tauri().event.listen("os://frame-login-closed", (e) => cb({ ...(e.payload as { id: string }), closed: true }));
    return () => [a, b].forEach((s) => void s.then((off) => off()));
  },
  takeOpenRequest: () => call("os_take_open_request"),
  onOpenRequest(cb) {
    const sub = tauri().event.listen("os://open-request", () => cb());
    return () => void sub.then((off) => off());
  },
  openLogin: () => call("os_open_login"),
  exit: () => call("os_exit"),
  onChange(cb) {
    const subs = ["os://app-state", "os://app-window-closed", "os://login-changed", "os://badge"].map((n) => tauri().event.listen(n, () => cb()));
    return () => subs.forEach((s) => void s.then((off) => off()));
  },
  onProgress(cb) {
    const sub = tauri().event.listen("os://armory-progress", (e) => cb(e.payload as ArmoryProgress));
    return () => void sub.then((off) => off());
  },
  osUpdateState: () => call("os_update_state"),
  osUpdateCheck: () => call("os_update_check"),
  osUpdateInstall: async () => {
    // Desktop: the host hands over to the installer and ends. Android (BOS-AND-SELFUPDATE-1): it answers the checked APK, which
    // goes to Android's installer here; when that does not install it, the apps start again and the reason is the error.
    const r = await call<{ apk?: string; sha256?: string } | null>("os_update_install");
    const c = updateChannel();
    if (!r?.apk || !c) return;
    const res = await androidInstall(c, r.apk, r.sha256 ?? "");
    if (res.ok) return;
    await call("os_update_abort").catch(() => {});
    throw new Error(res.error ?? "Android did not install the update.");
  },
  onOsUpdate(cb) {
    const sub = tauri().event.listen("os://os-update", (e) => cb(e.payload as OsUpdateInfo));
    return () => void sub.then((off) => off());
  },
  takeFirstRunNotice: () => call("os_take_first_run_notice"),
  onFirstRun(cb) {
    const sub = tauri().event.listen("os://first-run", (e) => cb(e.payload as FirstRunEvent));
    return () => void sub.then((off) => off());
  },
};

/** The host sends `fromCache` as the list of sources served from the cache (an empty list is "none"); the shell wants a flag. */
export function normalizeCatalog(c: Catalog): Catalog {
  const f = c.fromCache as unknown;
  return { ...c, fromCache: Array.isArray(f) ? f.length > 0 : Boolean(f) };
}

export const errText = (e: unknown): string => (typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));

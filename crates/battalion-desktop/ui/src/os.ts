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
}
export interface CatalogEntry { id: string; source: string; compatible: boolean; name?: string | null; publisher?: string | null; summary?: string | null; icon?: string | null; version?: string | null }
export interface Catalog { apps: CatalogEntry[]; rejected: unknown[]; sourceErrors: string[]; fromCache: boolean }
export type ArmoryOp = "install" | "update" | "rollback" | "remove";
/** `os://armory-progress`: bytes, total and percent only while the stage is `download`. */
export interface ArmoryProgress { id: string; op: ArmoryOp; stage: string; bytes?: number | null; total?: number | null; percent?: number | null }

export interface Os {
  listApps(): Promise<InstalledApp[]>;
  catalog(): Promise<Catalog>;
  install(id: string, channel?: string): Promise<void>;
  update(id: string): Promise<void>;
  remove(id: string): Promise<void>;
  rollback(id: string): Promise<void>;
  startApp(id: string): Promise<void>;
  openAppWindow(id: string): Promise<void>;
  openLogin(): Promise<void>;
  /** Ends the OS process only; running apps keep running. */
  exit(): Promise<void>;
  /** Calls [cb] on any change the host pushes; returns the unsubscribe. */
  onChange(cb: () => void): () => void;
  /** Install, update, rollback and remove progress; returns the unsubscribe. */
  onProgress(cb: (p: ArmoryProgress) => void): () => void;
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

export const hostOs: Os = {
  listApps: () => call("os_list_apps"),
  catalog: () => call("os_catalog"),
  install: async (id, channel) => void (await call("os_install_app", { id, channel })),
  update: async (id) => void (await call("os_update_app", { id })),
  remove: async (id) => void (await call("os_remove_app", { id })),
  rollback: async (id) => void (await call("os_rollback_app", { id })),
  startApp: async (id) => void (await call("os_start_app", { id })),
  openAppWindow: (id) => call("os_open_app_window", { id }),
  openLogin: () => call("os_open_login"),
  exit: () => call("os_exit"),
  onChange(cb) {
    const subs = ["os://app-state", "os://app-window-closed", "os://login-changed"].map((n) => tauri().event.listen(n, () => cb()));
    return () => subs.forEach((s) => void s.then((off) => off()));
  },
  onProgress(cb) {
    const sub = tauri().event.listen("os://armory-progress", (e) => cb(e.payload as ArmoryProgress));
    return () => void sub.then((off) => off());
  },
};

export const errText = (e: unknown): string => (typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));

// SPDX-License-Identifier: MPL-2.0
// The one place the shell talks to the Rust host: the frozen os_* commands (docs/commands.md) and events. Nothing else invokes.
export type AppStateName = "absent" | "downloading" | "verifying" | "installed" | "initialising" | "stopped" | "starting" | "running" | "stopping" | "updating" | "failed" | "removing";
export interface InstalledApp { id: string; version: string; state: AppStateName; channel: string }
export interface CatalogEntry { id: string; source: string; compatible: boolean }
export interface Catalog { apps: CatalogEntry[]; rejected: unknown[]; sourceErrors: string[]; fromCache: boolean }

export interface Os {
  listApps(): Promise<InstalledApp[]>;
  catalog(): Promise<Catalog>;
  install(id: string, channel?: string): Promise<void>;
  startApp(id: string): Promise<void>;
  openAppWindow(id: string): Promise<void>;
  openLogin(): Promise<void>;
  exit(): Promise<void>;
  /** Calls [cb] on any change the host pushes; returns the unsubscribe. */
  onChange(cb: () => void): () => void;
}

interface Tauri {
  core: { invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> };
  event: { listen(name: string, cb: () => void): Promise<() => void> };
  window: { getCurrentWindow(): { close(): Promise<void> } };
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
  startApp: async (id) => void (await call("os_start_app", { id })),
  openAppWindow: (id) => call("os_open_app_window", { id }),
  openLogin: () => call("os_open_login"),
  exit: () => tauri().window.getCurrentWindow().close(),
  onChange(cb) {
    const subs = ["os://app-state", "os://app-window-closed", "os://login-changed"].map((n) => tauri().event.listen(n, cb));
    return () => subs.forEach((s) => void s.then((off) => off()));
  },
};

export const errText = (e: unknown): string => (typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));

// SPDX-License-Identifier: Apache-2.0
// Copyright (c) SuperiorSoft LLC
// @battalionos/app-api: the typed client a BattalionOS app's page uses. `connect()` talks to the OS when the page runs in an
// OS frame, and to the local backend (an in-memory tree and store) in a plain browser, so an app is built without the OS.
// Docs: docs/apps/app-api.md. The calls: docs/commands.md. No framework dependency.
import { createBos, type Bos } from "./client";
import { localTransport, type LocalOptions } from "./local";
import { frameTransport } from "./transport";

export * from "./client";
export * from "./errors";
export * from "./transport";
export { localTransport, type LocalOptions } from "./local";
export type * from "./fs.gen";
export type * from "./store.gen";
export type * from "./window.gen";

export interface ConnectOptions {
  /** `auto` (default): the OS when the page is in a frame, else local. */
  backend?: "auto" | "os" | "local";
  /** Fail an OS call with no answer after this many ms (0 = wait). Default 30000. */
  timeoutMs?: number;
  /** Options of the local backend. */
  local?: LocalOptions;
}

/** The client for this page. */
export function connect(opts: ConnectOptions = {}): Bos {
  const w = globalThis as unknown as { window?: Window & typeof globalThis };
  const inFrame = !!w.window && w.window.parent !== w.window;
  const { backend: want = "auto", local } = opts;
  if (want === "os" || (want === "auto" && inFrame)) {
    if (!w.window) throw new Error("the OS backend needs a page (window)");
    return createBos(frameTransport(w.window.parent, w.window, { timeoutMs: opts.timeoutMs ?? 30000 }), "os");
  }
  return createBos(localTransport(local), "local");
}

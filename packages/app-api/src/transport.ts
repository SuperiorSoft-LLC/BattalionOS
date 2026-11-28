// SPDX-License-Identifier: Apache-2.0
// Copyright (c) SuperiorSoft LLC
// APP-API-1: how a call reaches the OS. A page in an OS frame sends `{bos: 1, id, cmd, args}` to its parent and gets
// `{bos: 1, id, ok, result | error}`; events (`os_fs_changed`, `os_win`, `os_notify_click`, ...) come as `{bos: 1, event, ...}`
// (docs/commands.md, "The page in a frame"). The same shape as the kit's window transport (WM-5).

export interface Transport {
  /** One call; resolves with the result, rejects with the OS's refusal text. */
  send(cmd: string, args: Record<string, unknown>): Promise<unknown>;
  /** Every event message the OS sends this page. Returns the unsubscribe. */
  listen(fn: (message: Record<string, unknown>) => void): () => void;
}

export interface MessageTarget {
  postMessage(message: unknown, targetOrigin: string): void;
}
export interface MessageSource {
  addEventListener(type: "message", fn: (e: { source?: unknown; data: unknown }) => void): void;
  removeEventListener(type: "message", fn: (e: { source?: unknown; data: unknown }) => void): void;
}

/** The frame bridge: `parent` is the OS shell, `self` the page's own window. Only messages from `parent` count. */
export function frameTransport(parent: MessageTarget, self: MessageSource, opts: { timeoutMs?: number } = {}): Transport {
  let n = 0;
  const pending = new Map<string, { ok: (v: unknown) => void; fail: (e: Error) => void }>();
  const listeners = new Set<(m: Record<string, unknown>) => void>();
  self.addEventListener("message", (e) => {
    if (e.source !== undefined && e.source !== parent) return;
    const d = e.data as Record<string, unknown> | null;
    if (!d || typeof d !== "object" || d.bos !== 1) return;
    if (d.event === undefined && typeof d.id === "string") {
      const p = pending.get(d.id);
      if (!p) return;
      pending.delete(d.id);
      if (d.ok === true) p.ok(d.result);
      else p.fail(new Error(typeof d.error === "string" ? d.error : "the OS refused the call"));
      return;
    }
    if (typeof d.event === "string") for (const l of listeners) l(d);
  });
  return {
    send(cmd, args) {
      const id = `app-api-${++n}`;
      return new Promise((ok, fail) => {
        pending.set(id, { ok, fail });
        if (opts.timeoutMs)
          setTimeout(() => {
            if (pending.delete(id)) fail(new Error(`${cmd}: no answer from the OS`));
          }, opts.timeoutMs);
        parent.postMessage({ bos: 1, id, cmd, args }, "*");
      });
    },
    listen(fn) {
      listeners.add(fn);
      return () => void listeners.delete(fn);
    },
  };
}

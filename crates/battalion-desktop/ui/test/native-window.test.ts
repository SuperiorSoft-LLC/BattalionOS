// SPDX-License-Identifier: MPL-2.0
// WM-6: a native window's host page: its identity, and the frame in it speaks for its own app and its own wid only.
import { describe, expect, it } from "vitest";
import { answer } from "../src/frames";
import { identity, nativeBackend } from "../src/nativeWindow";

const me = { app: "demo-app", wid: "w-demo-app-3", title: "Chat", origin: "http://127.0.0.1:4100", src: "http://127.0.0.1:4100/session/e7" };

describe("native window host page", () => {
  it("takes only an identity the host could have made", () => {
    expect(identity(me)).toEqual(me);
    expect(identity(null)).toBeNull();
    expect(identity({ ...me, wid: "w-other-3" })).toBeNull();
    expect(identity({ ...me, wid: "w-demo-app-x" })).toBeNull();
    expect(identity({ ...me, src: "https://evil.example/session" })).toBeNull();
  });

  it("the frame learns its wid and its window calls name this window", async () => {
    const calls: [string, unknown][] = [];
    const be = nativeBackend(me, async (cmd, args) => {
      calls.push([cmd, args]);
      return null;
    });
    const child = {};
    const frames = [{ id: me.app, origin: me.origin, window: child, wid: me.wid }];
    const ask = (cmd: string, args?: unknown) => answer(frames, { source: child, origin: me.origin, data: { bos: 1, id: "r1", cmd, args } }, be);

    expect((await ask("os_window_info"))?.message).toEqual({ bos: 1, id: "r1", ok: true, result: { label: "win-demo-app-3", app: "demo-app", wid: "w-demo-app-3" } });
    await ask("os_app_status");
    await ask("os_close_app_window");
    await ask("os_badge", { count: 4 });
    await ask("os_win_snap", { wid: me.wid, zone: "left" });
    expect(calls).toEqual([
      ["os_app_status", { id: "demo-app" }],
      ["os_win_close", { wid: me.wid }],
      ["os_win_set", { wid: me.wid, badge: { count: 4, text: undefined } }],
      ["os_win_snap", { wid: me.wid, zone: "left" }],
    ]);
    // Another app's id in the message is refused; a message from another origin gets nothing.
    expect((await ask("os_win_list", { id: "other" }))?.message).toMatchObject({ ok: false });
    expect(await answer(frames, { source: child, origin: "http://127.0.0.1:9", data: { bos: 1, id: "r2", cmd: "os_win_list" } }, be)).toBeNull();
    expect((await ask("os_pick_file"))?.message).toMatchObject({ ok: false, error: "'os_pick_file' is not available in a native window" });
  });
});

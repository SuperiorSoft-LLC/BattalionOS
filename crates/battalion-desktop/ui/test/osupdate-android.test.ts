// SPDX-License-Identifier: MPL-2.0
// BOS-AND-SELFUPDATE-1: the phone's own update in the shared Armory row: the confirm says Android asks once more, the one-line
// "is ready" notice, and the hand-off to Android's installer.
import { describe, expect, it } from "vitest";
import { androidInstall, confirmText, osReadyText, updateChannel, type OsUpdateInfo, type UpdateChannel } from "../src/osUpdate";

const info = (over: Partial<OsUpdateInfo> = {}): OsUpdateInfo => ({ current: "0.1.5", available: { version: "0.1.6", notes: "" }, installing: false, busy: [], running: [], ...over });

describe("BattalionOS on Android updates itself", () => {
  it("says Android shows its own confirm", () => {
    expect(confirmText(info({ confirmNote: "Android then shows its own Update confirm once." }))).toBe("BattalionOS restarts. Android then shows its own Update confirm once.");
    expect(confirmText(info())).toBe("BattalionOS restarts.");
  });
  it("shows one line while a newer version is ready, until put off for that version", () => {
    expect(osReadyText(info(), null)).toBe("BattalionOS 0.1.6 is ready");
    expect(osReadyText(info(), "0.1.6")).toBeNull();
    expect(osReadyText(info({ available: { version: "0.1.7", notes: "" } }), "0.1.6")).toBe("BattalionOS 0.1.7 is ready");
    expect(osReadyText(info({ available: null }), null)).toBeNull();
    expect(osReadyText(info({ installing: true }), null)).toBeNull();
  });
  it("finds the update channel only where the host gives it", () => {
    expect(updateChannel({})).toBeNull();
    const c = { postMessage() {}, onmessage: null };
    expect(updateChannel({ bosUpdate: c })).toBe(c);
  });
  it("hands the APK to Android and waits past the pending confirm for the answer", async () => {
    const sent: string[] = [];
    const c: UpdateChannel = { postMessage: (m) => sent.push(m), onmessage: null };
    const p = androidInstall(c, "/data/x.apk", "ab");
    expect(JSON.parse(sent[0]!)).toEqual({ op: "osInstall", apk: "/data/x.apk", sha256: "ab" });
    c.onmessage!({ data: JSON.stringify({ ev: "osInstall", pending: true }) });
    c.onmessage!({ data: JSON.stringify({ ev: "osInstall", ok: false, error: "You cancelled the update." }) });
    expect(await p).toEqual({ ok: false, error: "You cancelled the update." });
    expect(c.onmessage).toBeNull();
  });
});

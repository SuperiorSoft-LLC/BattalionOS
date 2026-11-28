// SPDX-License-Identifier: MPL-2.0
// ARMORY-CHANNEL-SWITCH: the channel picker on the app page.
import { afterEach, describe, expect, it, vi } from "vitest";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { ChannelPicker } from "../src/Armory";
import { hostOs, type InstalledApp } from "../src/os";
import { channelNoteOf, channelsOf, channelText, compareVersions, initial, reduce } from "../src/store";

const app = (over: Partial<InstalledApp>): InstalledApp => ({ id: "notes", version: "0.8.10", state: "stopped", channel: "stable", channels: ["nightly", "stable", "prerelease"], channelVersion: "0.8.10", ...over });
const model = (a: InstalledApp) => reduce(initial, { type: "installed-loaded", apps: [a] });

afterEach(() => vi.unstubAllGlobals());

describe("channel picker", () => {
  it("lists the catalog's channels: stable, prerelease, nightly first, then others", () => {
    expect(channelsOf(model(app({ channels: ["beta", "nightly", "stable", "prerelease"] })), "notes")).toEqual(["stable", "prerelease", "nightly", "beta"]);
    expect(channelsOf(model(app({ channels: ["stable"] })), "notes")).toEqual(["stable"]);
    expect(channelsOf(model(app({ channels: undefined })), "notes")).toEqual([]);
  });

  it("switching calls os_set_app_channel", async () => {
    const invoke = vi.fn(async () => null);
    vi.stubGlobal("__TAURI__", { core: { invoke }, event: { listen: async () => () => {} } });
    await hostOs.setChannel("notes", "prerelease");
    expect(invoke).toHaveBeenCalledWith("os_set_app_channel", { id: "notes", channel: "prerelease" });
  });

  it("a channel with no build says so", () => {
    const note = channelText(channelNoteOf(model(app({ channel: "nightly", channelVersion: null })), "notes"));
    expect(note).toBe("No builds on this channel yet");
    expect(renderToStaticMarkup(createElement(ChannelPicker, { id: "notes", channels: ["stable", "nightly"], channel: "nightly", note, onPick: () => {} }))).toContain("No builds on this channel yet");
  });

  it("a channel behind the installed version stays, never downgrades", () => {
    const note = channelText(channelNoteOf(model(app({ channel: "stable", channelVersion: "0.8.8" })), "notes"));
    expect(note).toBe("Stays on 0.8.10 until stable passes it");
    expect(renderToStaticMarkup(createElement(ChannelPicker, { id: "notes", channels: ["stable"], channel: "stable", note, onPick: () => {} }))).toContain("Stays on 0.8.10 until stable passes it");
    expect(channelNoteOf(model(app({ version: "0.8.8", channel: "prerelease", channelVersion: "0.8.10" })), "notes")).toBeNull();
  });

  it("compares versions numerically", () => {
    expect(compareVersions("0.8.10", "0.8.8")).toBe(1);
    expect(compareVersions("0.8.8", "0.8.8")).toBe(0);
    expect(compareVersions("1.0.0-rc.1", "1.0.0")).toBe(-1);
  });
});

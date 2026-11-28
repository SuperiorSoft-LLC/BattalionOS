// SPDX-License-Identifier: Apache-2.0
// The demo's OS calls on the local backend: what the page does, without a browser.
import { connect } from "@battalionos/app-api";
import { describe, expect, it } from "vitest";

describe("the demo's calls", () => {
  it("list, read, write and the remembered folder", async () => {
    const bos = connect({ backend: "local", local: { files: { "notes/a.txt": "a" } } });
    expect((await bos.fs.listAll("notes")).map((e) => e.name)).toEqual(["a.txt"]);
    await bos.fs.writeText("notes/a.txt", "edited");
    expect(await bos.fs.readText("notes/a.txt")).toBe("edited");
    await bos.store.set("lastDir", "notes");
    expect(await bos.store.get("lastDir")).toBe("notes");
  });
});

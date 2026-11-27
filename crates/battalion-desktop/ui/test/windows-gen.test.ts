// SPDX-License-Identifier: MPL-2.0
// WM-5: the TS window types are generated from battalion-app-api's window.schema.json and kept fresh.
// Regenerate: UPDATE_GEN=1 pnpm vitest run test/windows-gen.test.ts
import { describe, expect, it } from "vitest";
import schema from "../../../battalion-app-api/schemas/window.schema.json?raw";
import current from "../kit/lib/windows/types.gen.ts?raw";
import { generateWindowTypes } from "../kit/lib/windows/gen";

describe("window types", () => {
  it("types.gen.ts matches window.schema.json", async () => {
    const want = generateWindowTypes(schema);
    const env = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env ?? {};
    if (env.UPDATE_GEN) {
      const fs = (await import(/* @vite-ignore */ "node:" + "fs")) as { writeFileSync(p: URL, s: string): void };
      fs.writeFileSync(new URL("../kit/lib/windows/types.gen.ts", import.meta.url), want);
      return;
    }
    expect(current.replace(/\r\n/g, "\n")).toBe(want);
  });
});

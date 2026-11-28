// SPDX-License-Identifier: Apache-2.0
// APP-API-1: the package's types are generated from battalion-app-api's schemas and kept fresh (the schema is the contract).
// Regenerate: UPDATE_GEN=1 pnpm vitest run test/gen.test.ts
import { describe, expect, it } from "vitest";
import fsSchema from "../../../crates/battalion-app-api/schemas/fs.schema.json?raw";
import storeSchema from "../../../crates/battalion-app-api/schemas/store.schema.json?raw";
import windowSchema from "../../../crates/battalion-app-api/schemas/window.schema.json?raw";
import fsCurrent from "../src/fs.gen.ts?raw";
import storeCurrent from "../src/store.gen.ts?raw";
import windowCurrent from "../src/window.gen.ts?raw";
import { generateTypes } from "../src/gen";

const cases: [string, string, string][] = [
  ["fs", fsSchema, fsCurrent],
  ["store", storeSchema, storeCurrent],
  ["window", windowSchema, windowCurrent],
];

describe("generated types", () => {
  for (const [name, schema, current] of cases) {
    it(`${name}.gen.ts matches ${name}.schema.json`, async () => {
      const want = generateTypes(name, schema);
      const env = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env ?? {};
      if (env.UPDATE_GEN) {
        const fs = (await import(/* @vite-ignore */ "node:" + "fs")) as { writeFileSync(p: URL, s: string): void };
        fs.writeFileSync(new URL(`../src/${name}.gen.ts`, import.meta.url), want);
        return;
      }
      expect(current.replace(/\r\n/g, "\n")).toBe(want);
    });
  }
});

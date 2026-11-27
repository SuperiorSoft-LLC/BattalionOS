// SPDX-License-Identifier: Apache-2.0
// Copyright (c) SuperiorSoft LLC
// WM-5: turns battalion-app-api `schemas/window.schema.json` into `types.gen.ts`. It reads only the subset the schema
// uses ($defs of objects, enums, consts, oneOf, $ref, null). test/windows-gen.test.ts fails when the file is stale and
// rewrites it with UPDATE_GEN=1.

type Node = {
  type?: string;
  enum?: string[];
  const?: string;
  oneOf?: Node[];
  $ref?: string;
  required?: string[];
  properties?: Record<string, Node>;
};

function ts(n: Node): string {
  if (n.$ref) return n.$ref.replace("#/$defs/", "");
  if (n.enum) return n.enum.map((v) => JSON.stringify(v)).join(" | ");
  if (n.const !== undefined) return JSON.stringify(n.const);
  if (n.oneOf) return n.oneOf.map((o) => (o.type === "object" ? `(${ts(o)})` : ts(o))).join(" | ");
  switch (n.type) {
    case "object": {
      const req = new Set(n.required ?? []);
      const fields = Object.entries(n.properties ?? {}).map(([k, v]) => `${k}${req.has(k) ? "" : "?"}: ${ts(v)}`);
      return `{ ${fields.join("; ")} }`;
    }
    case "string":
      return "string";
    case "number":
    case "integer":
      return "number";
    case "boolean":
      return "boolean";
    case "null":
      return "null";
    default:
      throw new Error(`window.schema.json: unsupported node ${JSON.stringify(n)}`);
  }
}

export function generateWindowTypes(schemaText: string): string {
  const defs = (JSON.parse(schemaText) as { $defs: Record<string, Node> }).$defs;
  const lines = [
    "// SPDX-License-Identifier: Apache-2.0",
    "// GENERATED from crates/battalion-app-api/schemas/window.schema.json by kit/lib/windows/gen.ts. Do not edit:",
    "// change the schema (and battalion-app-api src/windows.rs), then run UPDATE_GEN=1 pnpm vitest run test/windows-gen.test.ts.",
    "",
  ];
  for (const [name, node] of Object.entries(defs)) lines.push(`export type ${name} = ${ts(node)};`);
  return lines.join("\n") + "\n";
}

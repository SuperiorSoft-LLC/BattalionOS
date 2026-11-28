// SPDX-License-Identifier: Apache-2.0
// Copyright (c) SuperiorSoft LLC
// APP-API-1: turns a battalion-app-api schema (crates/battalion-app-api/schemas/*.schema.json) into TS types, the WM-5 pattern.
// It reads only the subset the schemas use: $defs of objects, arrays, enums, consts, oneOf, $ref, null, and `{}` (any JSON).
// test/gen.test.ts fails when a generated file is stale and rewrites it with UPDATE_GEN=1.

type Node = {
  type?: string;
  enum?: (string | number)[];
  const?: string | number;
  oneOf?: Node[];
  $ref?: string;
  required?: string[];
  properties?: Record<string, Node>;
  additionalProperties?: Node;
  items?: Node;
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
      if (n.additionalProperties) fields.push(`[key: string]: ${ts(n.additionalProperties)}`);
      return `{ ${fields.join("; ")} }`;
    }
    case "array":
      return `${n.items?.$ref || n.items?.type ? ts(n.items) : "unknown"}[]`;
    case "string":
      return "string";
    case "number":
    case "integer":
      return "number";
    case "boolean":
      return "boolean";
    case "null":
      return "null";
    case undefined:
      return "unknown";
    default:
      throw new Error(`schema: unsupported node ${JSON.stringify(n)}`);
  }
}

/** The TS source for every `$defs` entry of `schemaText` (`name` is the schema file's, for the header). */
export function generateTypes(name: string, schemaText: string): string {
  const defs = (JSON.parse(schemaText) as { $defs: Record<string, Node> }).$defs;
  const lines = [
    "// SPDX-License-Identifier: Apache-2.0",
    `// GENERATED from crates/battalion-app-api/schemas/${name}.schema.json by src/gen.ts. Do not edit: change the schema,`,
    "// then run UPDATE_GEN=1 pnpm vitest run test/gen.test.ts.",
    "",
  ];
  for (const [def, node] of Object.entries(defs)) lines.push(`export type ${def} = ${ts(node)};`);
  return lines.join("\n") + "\n";
}

// SPDX-License-Identifier: Apache-2.0
// GENERATED from crates/battalion-app-api/schemas/open.schema.json by src/gen.ts. Do not edit: change the schema,
// then run UPDATE_GEN=1 pnpm vitest run test/gen.test.ts.

export type OpenCall = "os_open_with" | "os_open_file_take";
export type OpenErrorCode = "bad_args" | "no_such_app" | "no_app" | "bad_path" | "leaves_root" | "not_found" | "not_a_file" | "no_root" | "io";
export type OpenWithArgs = { path: string; appId?: string };
export type OpenCandidate = { id: string; name: string; label: string };
export type OpenWithResult = { opened: string | null; path: string };
export type OpenFileEvent = { bos: 1; event: "os_open_file"; path: string };
export type FileType = { label: string; extensions?: string[]; mime?: string[] };

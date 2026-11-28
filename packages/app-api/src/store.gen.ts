// SPDX-License-Identifier: Apache-2.0
// GENERATED from crates/battalion-app-api/schemas/store.schema.json by src/gen.ts. Do not edit: change the schema,
// then run UPDATE_GEN=1 pnpm vitest run test/gen.test.ts.

export type StoreCall = "os_store_get" | "os_store_set" | "os_store_delete" | "os_store_list";
export type StoreErrorCode = "bad_args" | "too_large" | "bad_app" | "io";
export type StoreValue = unknown;
export type StoreKeyArgs = { key: string };
export type StoreSetArgs = { key: string; value: StoreValue };
export type StoreListArgs = { prefix?: string };
export type StoreFile = { schema: 1; values: { [key: string]: StoreValue } };

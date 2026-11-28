// SPDX-License-Identifier: Apache-2.0
// GENERATED from crates/battalion-app-api/schemas/fs.schema.json by src/gen.ts. Do not edit: change the schema,
// then run UPDATE_GEN=1 pnpm vitest run test/gen.test.ts.

export type FsCall = "os_fs_list" | "os_fs_stat" | "os_fs_read" | "os_fs_watch" | "os_fs_unwatch" | "os_fs_write" | "os_fs_mkdir" | "os_fs_rename" | "os_fs_copy" | "os_fs_trash" | "os_fs_delete";
export type FsErrorCode = "no_root" | "bad_path" | "leaves_root" | "not_found" | "not_a_folder" | "not_a_file" | "not_text" | "too_many_watches" | "no_such_watch" | "bad_args" | "io" | "is_root" | "exists" | "too_large" | "not_empty";
export type FsKind = "file" | "dir" | "link";
export type FsEncoding = "text" | "base64";
export type FsEntry = { name: string; kind: FsKind; size: number; modified: number | null };
export type FsListArgs = { path?: string; cursor?: string; limit?: number };
export type FsList = { entries: FsEntry[]; cursor: string | null };
export type FsStat = { name: string; kind: FsKind; target: "file" | "dir"; size: number; modified: number | null; created: number | null; readonly: boolean; display: string };
export type FsReadArgs = { path: string; offset?: number; length?: number; as?: FsEncoding };
export type FsRead = { data: string; size: number; offset: number; length: number; eof: boolean };
export type FsWriteArgs = { path: string; data: string; as?: FsEncoding; overwrite?: boolean };
export type FsWrite = { size: number };
export type FsCopy = { entries: number };
export type FsWatch = { watch: string };
export type FsChange = "created" | "changed" | "removed";
export type FsChangedEvent = { bos: 1; event: "os_fs_changed"; watch: string; path: string | null; change: FsChange | null };

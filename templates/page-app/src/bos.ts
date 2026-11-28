// SPDX-License-Identifier: Apache-2.0
// The one client of this page. Under BattalionOS it talks to the OS; in a plain browser (`pnpm dev:browser`) it runs on the
// package's local backend: an in-memory file tree with a few sample files, and the store kept in localStorage.
import { connect } from "@battalionos/app-api";

export const bos = connect({
  local: {
    files: { "Welcome.md": "# Welcome\n\nThis file lives in the local backend.\n", "notes/todo.txt": "- try editing me\n" },
    storage: typeof localStorage === "undefined" ? undefined : localStorage,
    storageKey: "my-app",
  },
});

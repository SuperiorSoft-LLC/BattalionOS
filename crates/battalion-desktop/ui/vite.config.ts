// SPDX-License-Identifier: MPL-2.0
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { resolve } from "node:path";

// Built to ui/dist (tauri.conf.json frontendDist). login.html stays a static second page.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  base: "./",
  build: { outDir: "dist", rollupOptions: { input: { index: resolve(import.meta.dirname, "index.html"), login: resolve(import.meta.dirname, "login.html") } } },
  test: { include: ["test/**/*.test.ts"], environment: "node" },
});

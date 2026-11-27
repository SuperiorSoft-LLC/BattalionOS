// SPDX-License-Identifier: MPL-2.0
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { resolve } from "node:path";

// Built to ui/dist (tauri.conf.json frontendDist). login.html stays a static second page; window.html is the host page of a native
// window (WM-6).
export default defineConfig({
  plugins: [react(), tailwindcss()],
  base: "./",
  build: { outDir: "dist", rollupOptions: { input: { index: resolve(import.meta.dirname, "index.html"), login: resolve(import.meta.dirname, "login.html"), window: resolve(import.meta.dirname, "window.html") } } },
  // BOS-31: the dev server of the OS's own screens, on this PC's loopback only (never --host), on a port the dev script chooses.
  server: { host: "127.0.0.1", port: Number(process.env.BOS_UI_PORT ?? 1431), strictPort: true },
  test: { include: ["test/**/*.test.ts"], environment: "node" },
});

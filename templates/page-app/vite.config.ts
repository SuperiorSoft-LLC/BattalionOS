// SPDX-License-Identifier: Apache-2.0
// `pnpm dev`: the page on http://127.0.0.1:5173 with live reload (point BattalionOS at it, see README).
// `pnpm build`: the app folder `dist-app/` = app.json + ui/ (the page), the layout a .bosapp holds.
import react from "@vitejs/plugin-react";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { defineConfig } from "vite";

export default defineConfig({
  base: "./",
  plugins: [
    react(),
    {
      name: "bos-app-json",
      closeBundle() {
        mkdirSync("dist-app", { recursive: true });
        copyFileSync("app.json", "dist-app/app.json");
        if (existsSync("icon.png")) copyFileSync("icon.png", "dist-app/icon.png");
      },
    },
  ],
  server: { host: "127.0.0.1", port: 5173, strictPort: true },
  build: { outDir: "dist-app/ui", emptyOutDir: true },
});

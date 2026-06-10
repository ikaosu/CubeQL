// Vite config for the Tauri renderer.
//
// Differs from vite.renderer.config.ts (Electron):
//  - outputs to dist-tauri/ (which tauri.conf.json points to as frontendDist)
//  - uses port 1420 (Tauri default for `beforeDevCommand`)
//  - clears electron-specific globals (we're a plain browser environment under
//    Tauri; window.api is replaced by @tauri-apps/api invoke/listen).
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  root: ".",
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: false,
    watch: {
      ignored: ["**/rust-core/**"],
    },
  },
  build: {
    outDir: "dist-tauri",
    emptyOutDir: true,
    target: "es2022",
    sourcemap: true,
  },
});

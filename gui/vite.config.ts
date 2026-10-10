/**
 * Vite, with the `@/` alias.
 *
 * The alias is not a style preference. The layer checker resolves relative
 * specifiers by walking `..` segments, which is where it got its two bugs -- and
 * with `@/` the layer is the first segment, so the check reads `@/services/x` and
 * is right without any resolution at all.
 *
 * It also means a file can move without rewriting the imports that name it, which
 * matters in a tree where `views/` and `components/` are the same layer and files
 * cross between them freely.
 */

import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    // Tauri's own webview is Chromium or WebKitGTK; the ES2022 target matches both.
    target: "esnext",
    minify: "esbuild",
    sourcemap: false,
  },
});

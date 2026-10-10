import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri serves the built assets from a custom protocol in production and from a
// dev server in development, so the two paths differ. Every other Vite default
// is left alone deliberately: this is a single-page client over a local Rust
// process, and there is no code splitting to tune.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    // Tauri's own webview is Chromium or WebKitGTK; the ES2019 target matches
    // both and avoids the transpilation an `esnext` default would skip.
    target: "esnext",
    minify: "esbuild",
    sourcemap: false,
  },
});

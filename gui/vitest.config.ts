import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

// Vitest, in the same environment the client runs in, and with the same alias.
//
// The alias is repeated rather than shared because the three configs are read by
// three different tools that have no common entry point -- and a test that
// resolves `@/services` differently from the application it tests would be
// testing something else.
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  test: {
    environment: "jsdom",
    globals: false,
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["./src/test/dom.ts"],
  },
});

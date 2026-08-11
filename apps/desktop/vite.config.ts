import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed dev port; `clearScreen: false` keeps tauri CLI logs visible.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
    coverage: {
      provider: "v8",
      all: true,
      include: ["src/**/*.{ts,tsx}"],
      // main.tsx is DOM bootstrap wiring with no logic; it is exercised by
      // the Playwright suite, which loads the real bundle.
      exclude: ["src/main.tsx", "src/test/**", "src/**/*.test.{ts,tsx}"],
    },
  },
});

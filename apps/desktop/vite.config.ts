import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Wails expects a fixed dev port; `clearScreen: false` keeps its CLI logs visible.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
});

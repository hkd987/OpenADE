import { defineConfig, devices } from "@playwright/test";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

/**
 * End-to-end suite: real openade-daemon (cargo run) + real Vite dev server +
 * real Chromium. Harness CLIs are stand-in shims created by global-setup so
 * the full launch → PTY → attach flow runs without vendor credentials.
 */

import prepareWorld from "./e2e/global-setup";

const tmpDir = path.resolve(__dirname, "e2e/.tmp");
const daemonPort = 7455;
const uiPort = 5199;

// Build the fixture world NOW — the daemon webServer boots against these
// paths before any globalSetup hook would run. Guarded: this config is
// re-evaluated inside worker processes, which must NOT wipe the world the
// running daemon is using (workers inherit the runner's env).
if (process.env.OPENADE_E2E_PREPARED === undefined) {
  prepareWorld();
  process.env.OPENADE_E2E_PREPARED = "1";
}

// Local containers ship a pinned Chromium; CI installs one via
// `npx playwright install chromium`.
const pinnedChromium = "/opt/pw-browsers/chromium";
const executablePath =
  !process.env.CI && fs.existsSync(pinnedChromium) ? pinnedChromium : undefined;

export default defineConfig({
  testDir: "e2e",
  timeout: 60_000,
  expect: { timeout: 15_000 },
  // The suite mutates one shared daemon; keep it sequential.
  workers: 1,
  use: {
    baseURL: `http://127.0.0.1:${uiPort}`,
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
        launchOptions: executablePath !== undefined ? { executablePath } : {},
      },
    },
  ],
  webServer: [
    {
      command: "python3 e2e/mock-backstage.py",
      url: "http://127.0.0.1:7998/",
      timeout: 30_000,
      reuseExistingServer: false,
    },
    {
      command: "cargo run --quiet -p openade-daemon",
      cwd: path.resolve(__dirname, "../.."),
      url: `http://127.0.0.1:${daemonPort}/health`,
      timeout: 240_000,
      reuseExistingServer: false,
      env: {
        OPENADE_DAEMON_PORT: String(daemonPort),
        OPENADE_DATA_DIR: path.join(tmpDir, "data"),
        // The catalog layer talks to the mock Backstage above.
        BACKSTAGE_BASE_URL: "http://127.0.0.1:7998",
        // Harness shims (claude/codex/gemini) created by global-setup.
        PATH: `${path.join(tmpDir, "bin")}:${process.env.PATH ?? ""}`,
      },
    },
    {
      command: `npm run dev -- --port ${uiPort} --strictPort`,
      url: `http://127.0.0.1:${uiPort}`,
      timeout: 120_000,
      reuseExistingServer: false,
      env: {
        VITE_OPENADE_DAEMON_URL: `http://127.0.0.1:${daemonPort}`,
      },
    },
  ],
});

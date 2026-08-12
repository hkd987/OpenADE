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
// Second, unconfigured daemon + UI pair: exercises first-run onboarding.
const onboardingDaemonPort = 7456;
const onboardingUiPort = 5198;
// Self-hosted multiplayer workspace server (team.spec.ts).
const serverPort = 7501;

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
      // CI prebuilds the daemon in its own step; the generous timeout only
      // matters for cold local runs where `cargo run` compiles first.
      command: "cargo run --quiet -p openade-daemon",
      cwd: path.resolve(__dirname, "../.."),
      url: `http://127.0.0.1:${daemonPort}/health`,
      timeout: 600_000,
      reuseExistingServer: false,
      env: {
        OPENADE_DAEMON_PORT: String(daemonPort),
        OPENADE_DATA_DIR: path.join(tmpDir, "data"),
        // The catalog layer talks to the mock Backstage above.
        BACKSTAGE_BASE_URL: "http://127.0.0.1:7998",
        // Shared team memory repo, served by the gh shim's stateful
        // contents API (state under e2e/.tmp/team-memory).
        OPENADE_MEMORY_REPO: "acme/team-memory",
        // Harness shims (claude/codex/gemini) created by global-setup.
        PATH: `${path.join(tmpDir, "bin")}:${process.env.PATH ?? ""}`,
      },
    },
    {
      // First-run onboarding daemon: no memory env vars, no config file —
      // the UI must show the welcome flow. The gh shim is still on PATH
      // (zero-config GitHub memory) and serves the team-memory contents
      // API so saved settings are exercised end to end.
      command: "cargo run --quiet -p openade-daemon",
      cwd: path.resolve(__dirname, "../.."),
      url: `http://127.0.0.1:${onboardingDaemonPort}/health`,
      timeout: 600_000,
      reuseExistingServer: false,
      env: {
        OPENADE_DAEMON_PORT: String(onboardingDaemonPort),
        OPENADE_DATA_DIR: path.join(tmpDir, "data-onboarding"),
        PATH: `${path.join(tmpDir, "bin")}:${process.env.PATH ?? ""}`,
      },
    },
    {
      // Multiplayer workspace hub: a real openade-server with a fresh data
      // dir. team.spec.ts mints a member token over HTTP with the admin
      // token and configures the main daemon through the Settings UI.
      command: "cargo run --quiet -p openade-server",
      cwd: path.resolve(__dirname, "../.."),
      url: `http://127.0.0.1:${serverPort}/health`,
      timeout: 600_000,
      reuseExistingServer: false,
      env: {
        OPENADE_SERVER_PORT: String(serverPort),
        OPENADE_SERVER_BIND: "127.0.0.1",
        OPENADE_SERVER_DATA_DIR: path.join(tmpDir, "server-data"),
        OPENADE_SERVER_ADMIN_TOKEN: "e2e-admin",
      },
    },
    {
      // Bind 127.0.0.1 explicitly: on CI runners `localhost` can resolve to
      // ::1, and Playwright polls the IPv4 address.
      command: `npm run dev -- --host 127.0.0.1 --port ${uiPort} --strictPort`,
      url: `http://127.0.0.1:${uiPort}`,
      timeout: 120_000,
      reuseExistingServer: false,
      env: {
        VITE_OPENADE_DAEMON_URL: `http://127.0.0.1:${daemonPort}`,
      },
    },
    {
      command: `npm run dev -- --host 127.0.0.1 --port ${onboardingUiPort} --strictPort`,
      url: `http://127.0.0.1:${onboardingUiPort}`,
      timeout: 120_000,
      reuseExistingServer: false,
      env: {
        VITE_OPENADE_DAEMON_URL: `http://127.0.0.1:${onboardingDaemonPort}`,
      },
    },
  ],
});

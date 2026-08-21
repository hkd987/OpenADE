import { defineConfig, devices } from "@playwright/test";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import prepareWorld from "./e2e/global-setup";

const root = path.dirname(fileURLToPath(import.meta.url));
const tmpDir = path.resolve(root, "e2e/.tmp");
const daemonPort = 7455;
const uiPort = 5199;

if (process.env.OPENADE_E2E_PREPARED === undefined) {
  prepareWorld();
  process.env.OPENADE_E2E_PREPARED = "1";
}

const pinnedChromium = "/opt/pw-browsers/chromium";
const executablePath = !process.env.CI && fs.existsSync(pinnedChromium)
  ? pinnedChromium
  : undefined;

export default defineConfig({
  testDir: "e2e",
  testMatch: "ade-lifecycle.spec.ts",
  timeout: 60_000,
  expect: { timeout: 15_000 },
  workers: 1,
  fullyParallel: false,
  use: {
    baseURL: `http://127.0.0.1:${uiPort}`,
    trace: "retain-on-failure",
  },
  projects: [{
    name: "chromium",
    use: {
      ...devices["Desktop Chrome"],
      launchOptions: executablePath ? { executablePath } : {},
    },
  }],
  webServer: [
    {
      command: `npm run build && go build -o e2e/.tmp/openade-e2e . && exec e2e/.tmp/openade-e2e --daemon --addr 127.0.0.1:${daemonPort} --data-dir ${path.join(tmpDir, "data")}`,
      cwd: root,
      url: `http://127.0.0.1:${daemonPort}/api/health`,
      timeout: 180_000,
      reuseExistingServer: false,
      env: {
        PATH: `${path.join(tmpDir, "bin")}:${process.env.PATH ?? ""}`,
        SHELL: "/bin/sh",
      },
    },
    {
      command: `npm run dev -- --host 127.0.0.1 --port ${uiPort} --strictPort`,
      cwd: root,
      url: `http://127.0.0.1:${uiPort}`,
      timeout: 120_000,
      reuseExistingServer: false,
      env: {
        VITE_OPENADE_DAEMON_URL: `http://127.0.0.1:${daemonPort}`,
      },
    },
  ],
});

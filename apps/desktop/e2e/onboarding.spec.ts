import { expect, test } from "@playwright/test";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// A separate, unconfigured daemon + UI pair (see playwright.config.ts):
// no memory env vars, no config file — a genuine first run.
const ui = "http://127.0.0.1:5198";
const daemon = "http://127.0.0.1:7456";

test("first startup onboards the user and applies settings live", async ({
  page,
  request,
}) => {
  await page.goto(ui);

  // The welcome flow appears; the gh shim is authenticated, so GitHub
  // memory reports ready with nothing to configure.
  const onboarding = page.getByTestId("onboarding");
  await expect(onboarding).toBeVisible();
  await expect(page.getByTestId("ob-gh-status")).toContainText(
    "GitHub memory is ready",
  );

  // Save a shared team memory repo through the flow.
  await page.getByTestId("ob-memory-repo").fill("acme/team-memory");
  await page.getByTestId("ob-save").click();
  await expect(onboarding).not.toBeVisible();

  // Applied live and persisted: the daemon reports it, config.json exists,
  // and a reload doesn't re-onboard.
  const config = await (await request.get(`${daemon}/config`)).json();
  expect(config.onboarded).toBe(true);
  expect(config.memory_repo).toBe("acme/team-memory");
  expect(config.memory_sources).toContain("github");
  const configFile = path.resolve(
    __dirname,
    ".tmp/data-onboarding/config.json",
  );
  expect(fs.existsSync(configFile)).toBeTruthy();
  await page.reload();
  await expect(page.getByTestId("session-grid")).toBeVisible();
  await expect(page.getByTestId("onboarding")).toHaveCount(0);
});

import { expect, test } from "@playwright/test";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// README screenshot capture. Not part of the verification suite — run
// explicitly with:
//   OPENADE_SCREENSHOTS=1 npx playwright test e2e/screenshots.spec.ts
// against the same server world as the e2e suite; images land in docs/img.
test.skip(
  process.env.OPENADE_SCREENSHOTS === undefined,
  "screenshot capture runs only with OPENADE_SCREENSHOTS=1",
);

const repo = path.resolve(__dirname, ".tmp/fixture-repo");
const daemon = "http://127.0.0.1:7455";
const server = "http://127.0.0.1:7501";
const admin = { Authorization: "Bearer e2e-admin" };
const img = (name: string) =>
  path.resolve(__dirname, "../../../docs/img", name);

test.describe.configure({ mode: "serial" });
test.use({ viewport: { width: 1440, height: 900 } });

test("capture README screenshots", async ({ page, request }) => {
  // Team world: workspace + member token, daemon connected via config.
  const minted = await request.post(`${server}/tokens`, {
    headers: admin,
    data: { name: "casey" },
  });
  const token = (await minted.json()).token as string;
  const ws = await request.post(`${server}/workspaces`, {
    headers: admin,
    data: { title: "Acme Eng", description: "Shared agent sessions" },
  });
  const workspaceId = (await ws.json()).id as number;
  await request.put(`${daemon}/config`, {
    data: {
      onboarded: true,
      server_url: server,
      server_token: token,
      server_workspace: workspaceId,
    },
  });

  // A couple of sessions worth sharing.
  await page.goto("/");
  await page.getByTestId("toggle-new-session").click();
  await page.getByTestId("ns-title").fill("Add a retry budget to checkout");
  await page.getByTestId("ns-repo").fill(repo);
  await page
    .getByTestId("ns-prompt")
    .fill("Add a retry budget to the checkout poller and cover it with tests");
  await page.screenshot({ path: img("new-session-form.png") });
  await page.getByTestId("ns-submit").click();
  await expect(page.getByTestId("terminal-view")).toContainText(
    "claude-shim started",
  );
  await page.getByTestId("share-button").click();
  await expect(page.getByTestId("share-banner")).toBeVisible();

  await page.getByTestId("toggle-new-session").click();
  await page.getByTestId("ns-title").fill("Harden the payment webhooks");
  await page.getByTestId("ns-harness").selectOption("opencode");
  await page.getByTestId("ns-repo").fill(repo);
  await page
    .getByTestId("ns-prompt")
    .fill("Verify webhook signatures and add replay protection");
  await page.getByTestId("ns-submit").click();
  await expect(page.getByTestId("terminal-view")).toContainText(
    "opencode-shim started",
  );
  await page.getByTestId("share-button").click();
  await expect(page.getByTestId("share-banner")).toBeVisible();

  // Team view: the shared history.
  await page.getByTestId("view-team").click();
  await expect(page.getByTestId("team-row")).toHaveCount(2);
  await page.screenshot({ path: img("team-view.png") });

  // Pickup: open the record, aim it at a different harness.
  await page.getByTestId("team-row").first().click();
  await expect(page.getByTestId("team-detail")).toBeVisible();
  await page.getByTestId("pickup-harness").selectOption("copilot-cli");
  await expect(page.getByTestId("pickup-repo")).toHaveValue(repo);
  await page.screenshot({ path: img("team-pickup.png") });
});

test("capture the onboarding screenshot", async ({ page }) => {
  // The unconfigured daemon + UI pair shows the welcome flow (now with
  // the multiplayer section).
  await page.goto("http://127.0.0.1:5198");
  await expect(page.getByTestId("onboarding")).toBeVisible();
  await page.screenshot({ path: img("onboarding.png") });
});

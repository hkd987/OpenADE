import { expect, test } from "@playwright/test";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const repo = path.resolve(__dirname, ".tmp/fixture-repo");
const daemon = "http://127.0.0.1:7455";
const server = "http://127.0.0.1:7501";
const admin = { Authorization: "Bearer e2e-admin" };

// Multiplayer, end to end against a REAL self-hosted openade-server:
// mint a member token as the admin, configure the daemon through the
// Settings UI, share a session, browse it in the Team view, and pick it
// up into a different harness. Sequential — one story, one workspace.
test.describe.configure({ mode: "serial" });

let memberToken = "";
let workspaceId = 0;

test("admin mints a member token and creates the team workspace", async ({
  request,
}) => {
  const health = await request.get(`${server}/health`);
  expect(health.ok()).toBeTruthy();

  // Bad tokens are rejected before configuring anything.
  const denied = await request.get(`${server}/whoami`, {
    headers: { Authorization: "Bearer junk" },
  });
  expect(denied.status()).toBe(401);

  const minted = await request.post(`${server}/tokens`, {
    headers: admin,
    data: { name: "casey" },
  });
  expect(minted.ok()).toBeTruthy();
  memberToken = (await minted.json()).token as string;
  expect(memberToken).toMatch(/^oadk_/);

  const ws = await request.post(`${server}/workspaces`, {
    headers: admin,
    data: { title: "Acme Eng", description: "Shared sessions" },
  });
  expect(ws.status()).toBe(201);
  workspaceId = (await ws.json()).id as number;
});

test("the settings dialog connects the daemon to the workspace", async ({
  page,
}) => {
  await page.goto("/");
  // Unconfigured: the Team view explains how to turn multiplayer on.
  await page.getByTestId("view-team").click();
  await expect(page.getByTestId("team-unconfigured")).toContainText(
    "openade-server",
  );

  await page.getByTestId("settings-button").click();
  await page.getByTestId("ob-server-url").fill(server);
  await page.getByTestId("ob-server-token").fill(memberToken);
  await page.getByTestId("ob-server-workspace").fill(String(workspaceId));
  await page.getByTestId("ob-save").click();
  await expect(page.getByTestId("onboarding")).not.toBeVisible();

  // Configured now — the Team view shows the (empty) shared history.
  await page.getByTestId("view-team").click();
  await expect(page.getByTestId("team-empty")).toContainText(
    "No shared sessions yet",
  );
});

test("sharing a session publishes it to the team workspace", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByTestId("toggle-new-session").click();
  await page.getByTestId("ns-title").fill("team retry fix");
  await page.getByTestId("ns-repo").fill(repo);
  await page.getByTestId("ns-prompt").fill("fix the retry bug");
  await page.getByTestId("ns-submit").click();
  await expect(page.getByTestId("terminal-view")).toContainText(
    "claude-shim started",
  );

  await page.getByTestId("share-button").click();
  await expect(page.getByTestId("share-banner")).toContainText(
    "Shared to the team workspace",
  );

  // The record landed on the real server, harness-neutral.
  const listed = await fetch(
    `${server}/workspaces/${workspaceId}/sessions`,
    { headers: { Authorization: `Bearer ${memberToken}` } },
  ).then((r) => r.json());
  expect(listed.sessions).toHaveLength(1);
  expect(listed.sessions[0].title).toBe("team retry fix");
  expect(listed.sessions[0].harness).toBe("claude-code");
  expect(listed.sessions[0].shared_by).toBe("casey");
});

test("the Team view browses the shared record", async ({ page }) => {
  await page.goto("/");
  await page.getByTestId("view-team").click();

  const row = page.getByTestId("team-row");
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("team retry fix");
  await expect(row).toContainText("casey");
  await expect(row).toContainText("Claude Code");
  await expect(row).toContainText("checkout-service");

  // Read-only viewer: the artifact markdown and the original transcript.
  await row.click();
  await expect(page.getByTestId("team-markdown")).toContainText(
    "team retry fix",
  );
  await expect(page.getByTestId("team-transcript")).toContainText(
    "fix the retry bug",
  );
  await page.getByTestId("team-back").click();
  await expect(page.getByTestId("team-row")).toBeVisible();
});

test("anyone can pick the session up in a different harness", async ({
  page,
  request,
}) => {
  await page.goto("/");
  await page.getByTestId("view-team").click();
  await page.getByTestId("team-row").click();

  // Pick it up in Copilot CLI — the record was shared from Claude Code.
  await page.getByTestId("pickup-harness").selectOption("copilot-cli");
  await page.getByTestId("pickup-repo").fill(repo);
  await page.getByTestId("pickup-button").click();

  // Back on Sessions with the new local session attached and running the
  // chosen harness's CLI.
  await expect(page.getByTestId("session-grid")).toBeVisible();
  await expect(page.getByTestId("terminal-view")).toContainText(
    "copilot-shim started",
  );
  await expect(page.getByTestId("terminal-view")).toContainText("pickup.md");

  // The daemon rendered the neutral record into the target harness:
  // its own worktree, a pickup takeover doc, and that harness's rules file.
  const sessions = await (await request.get(`${daemon}/sessions`)).json();
  const picked = sessions.sessions.find(
    (s: { title: string }) => s.title === "pickup: team retry fix",
  );
  expect(picked.harness).toBe("copilot-cli");
  const worktree = picked.worktree_path as string;
  const doc = fs.readFileSync(
    path.join(worktree, ".openade/pickup.md"),
    "utf8",
  );
  expect(doc).toContain("team retry fix");
  expect(doc).toContain("casey");
  expect(doc).toContain("fix the retry bug");
  expect(fs.existsSync(path.join(worktree, "AGENTS.md"))).toBeTruthy();
});

test("a revoked token turns into an actionable Team view error", async ({
  page,
  request,
}) => {
  // Revoke the member token server-side (admin op).
  const tokens = await (
    await request.get(`${server}/tokens`, { headers: admin })
  ).json();
  const casey = tokens.tokens.find(
    (t: { name: string }) => t.name === "casey",
  );
  const revoked = await request.post(
    `${server}/tokens/${casey.id}/revoke`,
    { headers: admin },
  );
  expect(revoked.status()).toBe(204);

  await page.goto("/");
  await page.getByTestId("view-team").click();
  await expect(page.getByTestId("team-error")).toContainText(
    "check your member token in Settings",
  );

  // Following its advice fixes it: mint a fresh token, paste it in
  // Settings, and the Team view works again.
  const minted = await request.post(`${server}/tokens`, {
    headers: admin,
    data: { name: "casey" },
  });
  memberToken = (await minted.json()).token as string;
  await page.getByTestId("settings-button").click();
  await page.getByTestId("ob-server-token").fill(memberToken);
  await page.getByTestId("ob-save").click();
  await expect(page.getByTestId("onboarding")).not.toBeVisible();
  // Leave and re-enter the Team view so it refetches with the new token.
  await page.getByTestId("view-sessions").click();
  await page.getByTestId("view-team").click();
  await expect(page.getByTestId("team-row")).toBeVisible();
});

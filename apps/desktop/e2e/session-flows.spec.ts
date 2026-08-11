import { expect, test } from "@playwright/test";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const repo = path.resolve(__dirname, ".tmp/fixture-repo");
const daemon = "http://127.0.0.1:7455";

// One continuous story against one daemon: launch → attach → work →
// diff/files → artifact → handoff → kill. Sequential by design.
test.describe.configure({ mode: "serial" });

test("grid starts empty and the daemon is reachable", async ({
  page,
  request,
}) => {
  const health = await request.get(`${daemon}/health`);
  expect(health.ok()).toBeTruthy();

  await page.goto("/");
  await expect(page.getByTestId("empty-grid")).toBeVisible();
});

test("launching a session from the form attaches a live terminal", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByTestId("toggle-new-session").click();
  await page.getByTestId("ns-title").fill("e2e task");
  await page.getByTestId("ns-repo").fill(repo);
  await page.getByTestId("ns-prompt").fill("do the e2e thing");
  await page.getByTestId("ns-submit").click();

  // The card shows up running, the detail pane attaches, and the PTY
  // output (from the claude shim) reaches the terminal.
  const card = page.locator(".session-card");
  await expect(card).toHaveCount(1);
  await expect(card.locator(".state")).toHaveText("running");
  await expect(page.getByTestId("terminal-view")).toContainText(
    "claude-shim started",
  );
  // The initial prompt was passed through to the harness CLI.
  await expect(page.getByTestId("terminal-view")).toContainText(
    "do the e2e thing",
  );
});

test("diff and file views reflect worktree changes", async ({
  page,
  request,
}) => {
  // The agent (us) edits a tracked file in the session's worktree.
  const sessions = await (await request.get(`${daemon}/sessions`)).json();
  const worktree = sessions.sessions[0].worktree_path as string;
  fs.writeFileSync(path.join(worktree, "README.md"), "fixture\ne2e change\n");

  await page.goto("/");
  await page.locator(".session-card").first().click();

  await page.getByTestId("tab-diff").click();
  await expect(page.getByTestId("diff-view")).toContainText("+e2e change");

  await page.getByTestId("tab-files").click();
  const files = page.getByTestId("file-list");
  await expect(files).toContainText("README.md");
  // Rules were materialized into the worktree for the harness.
  await expect(files).toContainText("CLAUDE.md");
});

test("terminal input reaches the harness process", async ({ page }) => {
  await page.goto("/");
  await page.locator(".session-card").first().click();
  await page.getByTestId("tab-terminal").click();

  await page.locator(".terminal-container").click();
  await page.keyboard.type("hello-agent");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("terminal-view")).toContainText(
    "got:hello-agent",
  );
});

test("knowledge artifact lands on a review branch", async ({ page }) => {
  await page.goto("/");
  await page.locator(".session-card").first().click();
  await page.getByTestId("artifact-button").click();

  const banner = page.getByTestId("artifact-banner");
  await expect(banner).toBeVisible();
  await expect(banner).toContainText("openade/knowledge-");
  await expect(banner).toContainText("docs/openade/sessions/");
});

test("handoff moves the task to another harness in the same worktree", async ({
  page,
  request,
}) => {
  await page.goto("/");
  await page.locator(".session-card").first().click();
  await page.getByTestId("handoff-target").selectOption("gemini-cli");
  await page.getByTestId("handoff-button").click();

  // Two sessions now: the ended claude one and the live gemini one.
  await expect(page.locator(".session-card")).toHaveCount(2);
  await expect(page.getByTestId("terminal-view")).toContainText(
    "gemini-shim started",
  );
  // The handoff prompt tells the new harness where to pick up.
  await expect(page.getByTestId("terminal-view")).toContainText(
    "handoff.md",
  );

  // Same worktree on both sessions.
  const sessions = await (await request.get(`${daemon}/sessions`)).json();
  const worktrees = new Set(
    sessions.sessions.map(
      (s: { worktree_path: string }) => s.worktree_path,
    ),
  );
  expect(worktrees.size).toBe(1);
});

test("killing a session marks it failed in the grid", async ({ page }) => {
  await page.goto("/");
  // Select the running (gemini) session.
  const running = page
    .locator(".session-card", { hasText: "running" })
    .first();
  await running.click();
  await page.getByTestId("kill-button").click();
  await expect(
    page.locator(".session-card .state", { hasText: "failed" }),
  ).toHaveCount(1);
});

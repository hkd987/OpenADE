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
  request,
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

  // Zero-config memory: no entity was typed, so the session auto-grounded
  // in the repo's own GitHub origin remote — the chip appears and the
  // worktree carries the gh-built context bundle.
  const chip = card.getByTestId("entity-chip");
  await expect(chip).toContainText("repo");
  await expect(chip).toContainText("acme/checkout-service");
  const sessions = await request.get(`${daemon}/sessions`);
  const worktree = (await sessions.json()).sessions[0]
    .worktree_path as string;
  const rules = fs.readFileSync(path.join(worktree, "CLAUDE.md"), "utf8");
  expect(rules).toContain("acme/checkout-service");
  expect(rules).toContain("group:acme/checkout-team");
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

  // Shared team memory: the artifact was also pushed straight to the
  // configured memory repo's default branch (gh shim state on disk).
  const sharedLink = page.getByTestId("shared-memory-link");
  await expect(sharedLink).toBeVisible();
  await expect(sharedLink).toContainText("acme/team-memory");

  const teamMemory = path.resolve(__dirname, ".tmp/team-memory");
  const index = fs.readFileSync(path.join(teamMemory, "index.md"), "utf8");
  expect(index).toContain("# Session knowledge index");
  const sessionDocs = fs.readdirSync(path.join(teamMemory, "sessions"));
  expect(sessionDocs.length).toBeGreaterThan(0);
  const doc = fs.readFileSync(
    path.join(teamMemory, "sessions", sessionDocs[0]),
    "utf8",
  );
  expect(doc).toContain("# Session:");
  expect(index).toContain(`](sessions/${sessionDocs[0]})`);
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

test("reopening the app reattaches sessions with full scrollback", async ({
  page,
}) => {
  // R1 acceptance: the UI is just a viewer — closing and reopening it must
  // reattach every session with its history intact.
  await page.goto("/");
  await page.locator(".session-card").first().click();
  await expect(page.getByTestId("terminal-view")).toContainText(
    "gemini-shim started",
  );
  // "Close the app": navigate away entirely, then come back.
  await page.goto("about:blank");
  await page.goto("/");
  await page.locator(".session-card").first().click();
  await expect(page.getByTestId("terminal-view")).toContainText(
    "gemini-shim started",
  );
});

test("entity-launched sessions carry catalog context", async ({
  page,
  request,
}) => {
  await page.goto("/");
  await page.getByTestId("toggle-new-session").click();
  await page.getByTestId("ns-title").fill("entity task");
  await page.getByTestId("ns-repo").fill(repo);
  await page
    .getByTestId("ns-entity")
    .fill("component:default/payments-api");
  await page.getByTestId("ns-submit").click();

  // The card shows the entity as a memory chip, and the harness rules file
  // received the context bundle built from the mock Backstage.
  const card = page.locator(".session-card", { hasText: "entity task" });
  const chip = card.getByTestId("entity-chip");
  await expect(chip).toContainText("component");
  await expect(chip).toContainText("default/payments-api");

  const sessions = await (
    await request.get(`${daemon}/sessions?entity=component:default/payments-api`)
  ).json();
  expect(sessions.sessions.length).toBe(1);
  const worktree = sessions.sessions[0].transcript_path
    ? // indexed record: fetch live meta for the worktree path
      (await (await request.get(`${daemon}/sessions/${sessions.sessions[0].id}`)).json())
        .worktree_path
    : sessions.sessions[0].worktree_path;
  const rules = fs.readFileSync(path.join(worktree, "CLAUDE.md"), "utf8");
  expect(rules).toContain("# System context: Payments API");
  expect(rules).toContain("Payments Team");
  // Catalog MCP server auto-registered for the session.
  expect(fs.existsSync(path.join(worktree, ".mcp.json"))).toBeTruthy();
});

test("repo-entity sessions carry GitHub memory via the gh CLI", async ({
  page,
  request,
}) => {
  await page.goto("/");
  await page.getByTestId("toggle-new-session").click();
  await page.getByTestId("ns-title").fill("repo task");
  await page.getByTestId("ns-repo").fill(repo);
  await page.getByTestId("ns-entity").fill("repo:acme/checkout-service");
  await page.getByTestId("ns-submit").click();

  // The card shows the repo memory chip (kind highlighted).
  const card = page.locator(".session-card", { hasText: "repo task" });
  const chip = card.getByTestId("entity-chip");
  await expect(chip).toContainText("repo");
  await expect(chip).toContainText("acme/checkout-service");

  // The worktree rules carry context built from the gh shim: repo
  // description + CODEOWNERS-derived team ownership. The entity filter
  // also includes the earlier auto-grounded sessions — pick this one by
  // title.
  const sessions = await (
    await request.get(`${daemon}/sessions?entity=repo:acme/checkout-service`)
  ).json();
  const record = sessions.sessions.find(
    (s: { title: string }) => s.title === "repo task",
  );
  expect(record).toBeDefined();
  const meta = await (
    await request.get(`${daemon}/sessions/${record.id}`)
  ).json();
  const rules = fs.readFileSync(
    path.join(meta.worktree_path, "CLAUDE.md"),
    "utf8",
  );
  expect(rules).toContain("acme/checkout-service");
  expect(rules).toContain("Checkout flow service for the acme shop.");
  expect(rules).toContain("group:acme/checkout-team");
  expect(
    fs.existsSync(path.join(meta.worktree_path, ".mcp.json")),
  ).toBeTruthy();
});

test("a session waiting on input shows needs-input in the grid", async ({
  page,
}) => {
  await page.goto("/");
  const card = page.locator(".session-card", { hasText: "entity task" });
  await card.click();
  await page.getByTestId("tab-terminal").click();
  await page.locator(".terminal-container").click();
  // The shim echoes the line back; the echoed text ends with '?' and then
  // the PTY goes quiet — the daemon flips the state to needs-input.
  await page.keyboard.type("May I proceed?");
  await page.keyboard.press("Enter");
  await expect(card.locator(".state")).toHaveText("needs-input", {
    timeout: 20_000,
  });
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

test("header affordances: palette, projects view, settings", async ({
  page,
}) => {
  await page.goto("/");
  // The header health dot reports the connected daemon.
  await expect(page.getByTestId("daemon-health")).toHaveClass(/ok/);

  // ⌘K/Ctrl+K palette jumps to a session by typing.
  await page.keyboard.press("Control+k");
  await expect(page.getByTestId("palette")).toBeVisible();
  await page.getByTestId("palette-input").fill("repo task");
  await page.getByTestId("palette-input").press("Enter");
  await expect(page.getByTestId("palette")).toHaveCount(0);
  await expect(page.getByTestId("session-detail")).toContainText("repo task");

  // Projects view aggregates per-repo state counts.
  await page.getByTestId("view-projects").click();
  const card = page.getByTestId("project-card");
  await expect(card).toContainText("fixture-repo");
  await expect(card).toContainText("running");
  await page.getByTestId("view-sessions").click();

  // Settings opens prefilled from the live daemon config and cancels.
  await page.getByTestId("settings-button").click();
  await expect(page.getByTestId("onboarding")).toBeVisible();
  await expect(page.getByTestId("ob-memory-repo")).toHaveValue(
    "acme/team-memory",
  );
  await page.getByTestId("ob-cancel").click();
  await expect(page.getByTestId("onboarding")).toHaveCount(0);
});

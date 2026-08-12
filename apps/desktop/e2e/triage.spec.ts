import { expect, test } from "@playwright/test";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const repo = path.resolve(__dirname, ".tmp/fixture-repo");
const daemon = "http://127.0.0.1:7455";
const server = "http://127.0.0.1:7501";
const admin = { Authorization: "Bearer e2e-admin" };

// NOTE: this file's name must sort AFTER team.spec.ts (t-e < t-r): the
// serial world's main daemon is workspace-configured by the team flows,
// so the inbox here is the shared REMOTE one. Signals → inbox → dismiss →
// 3× escalation → accept → triage session → outcome memory, end to end
// against the real openade-server.
test.describe.configure({ mode: "serial" });

function signal(affected: number) {
  return {
    source: "sentry",
    source_ref: "E-42",
    kind: "exception",
    severity: "critical",
    title: "NPE in checkout poller",
    body: "TypeError: cannot read x of undefined",
    evidence: [
      {
        kind: "stack_trace",
        label: "sentry trace",
        url: "https://sentry.example/e/42",
      },
    ],
    join_keys: { release: "v2.3.0" },
    affected_count: affected,
  };
}

let memberToken = "";

test("a posted signal lands in every member's inbox with a badge", async ({
  page,
  request,
}) => {
  const minted = await request.post(`${server}/tokens`, {
    headers: admin,
    data: { name: "casey" },
  });
  memberToken = (await minted.json()).token as string;

  // Any tool can post the documented schema (here: straight to the
  // server, like a Sentry webhook would).
  const res = await request.post(`${server}/signals`, {
    headers: { Authorization: `Bearer ${memberToken}` },
    data: signal(10),
  });
  expect((await res.json()).inserted).toBe(1);

  await page.goto("/");
  await expect(page.getByTestId("inbox-count")).toHaveText("1");
  await page.getByTestId("view-inbox").click();
  const row = page.getByTestId("inbox-row");
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("NPE in checkout poller");
  await expect(row).toContainText("critical");
  await expect(row).toContainText("affected 10");
});

test("dismissal is recorded and a 3x recurrence escalates back", async ({
  page,
  request,
}) => {
  await page.goto("/");
  await page.getByTestId("view-inbox").click();
  await page.getByTestId("inbox-row").click();

  // Evidence is one click deep; dismissal explains the memory loop.
  await expect(
    page.getByRole("link", { name: /sentry trace/ }),
  ).toHaveAttribute("href", "https://sentry.example/e/42");
  await page.getByTestId("dismiss-button").click();
  await expect(page.getByTestId("dismiss-dialog")).toContainText(
    "recorded in outcome memory",
  );
  await page.getByTestId("dismiss-intended_behavior").click();
  await expect(page.getByTestId("inbox-dismissed")).toContainText(
    "Dismissed (intended_behavior) by casey",
  );

  // The same fingerprint comes back 3× bigger → the item reopens.
  const res = await request.post(`${server}/signals`, {
    headers: { Authorization: `Bearer ${memberToken}` },
    data: signal(30),
  });
  expect((await res.json()).escalated).toBe(1);
  await page.getByTestId("view-sessions").click();
  await page.getByTestId("view-inbox").click();
  await expect(page.getByTestId("inbox-row").first()).toContainText(
    "affected 30",
  );
  await page.getByTestId("inbox-row").first().click();
  // The dismissal stays visible in outcome memory even after the reopen.
  await expect(page.getByTestId("inbox-outcomes")).toContainText("dismissed");
  await expect(page.getByTestId("inbox-outcomes")).toContainText(
    "intended_behavior",
  );
});

test("accepting starts a triage session carrying the evidence", async ({
  page,
  request,
}) => {
  await page.goto("/");
  await page.getByTestId("view-inbox").click();
  await page.getByTestId("inbox-row").first().click();

  await page.getByTestId("triage-harness").selectOption("opencode");
  await page.getByTestId("triage-repo").fill(repo);
  await page.getByTestId("accept-button").click();

  // Back on Sessions: the triage session runs the chosen harness with a
  // prompt pointing at the evidence doc.
  await expect(page.getByTestId("session-grid")).toBeVisible();
  await expect(page.getByTestId("terminal-view")).toContainText(
    "opencode-shim started",
  );
  await expect(page.getByTestId("terminal-view")).toContainText(
    "inbox-item.md",
  );
  const sessions = await (await request.get(`${daemon}/sessions`)).json();
  const triage = sessions.sessions.find(
    (s: { title: string }) => s.title === "triage: NPE in checkout poller",
  );
  expect(triage.inbox_item_id).toBeGreaterThan(0);
  const doc = fs.readFileSync(
    path.join(triage.worktree_path as string, ".openade/inbox-item.md"),
    "utf8",
  );
  expect(doc).toContain("https://sentry.example/e/42");
  expect(doc).toContain("v2.3.0");
  expect(doc).toContain("Prior outcomes on this fingerprint");
  expect(doc).toContain('"dismissed"');

  // Everyone sees who took it.
  await page.getByTestId("view-inbox").click();
  await expect(page.getByTestId("inbox-taken")).toContainText(
    "Accepted by casey",
  );
});

test("the local inbox works with no server configured", async ({
  page,
  request,
}) => {
  // The onboarding daemon (7456) has no workspace server: signals posted
  // to the DAEMON land in its embedded inbox — same surface, no login,
  // no configuration.
  const local = "http://127.0.0.1:7456";
  const config = await (await request.get(`${local}/config`)).json();
  expect(config.inbox_backend).toBe("local");
  const res = await request.post(`${local}/signals`, {
    data: {
      source: "ci",
      kind: "regression",
      severity: "medium",
      title: "flaky nightly build",
    },
  });
  expect((await res.json()).inserted).toBe(1);

  await page.goto("http://127.0.0.1:5198");
  // First-run onboarding may cover the app; skip it if present.
  if (await page.getByTestId("ob-skip").isVisible().catch(() => false)) {
    await page.getByTestId("ob-skip").click();
  }
  await page.getByTestId("view-inbox").click();
  await expect(page.getByTestId("inbox-row")).toContainText(
    "flaky nightly build",
  );
});

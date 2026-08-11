import { execFileSync } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const tmpDir = path.resolve(__dirname, ".tmp");

function git(cwd: string, ...args: string[]) {
  execFileSync("git", ["-C", cwd, ...args], { stdio: "pipe" });
}

/**
 * Prepares a clean world for each run:
 * - a fixture git repository sessions are launched in,
 * - fake harness CLIs (claude/codex/gemini) on the daemon's PATH,
 * - an empty daemon data dir.
 *
 * Called from playwright.config.ts at module-load time: it must run BEFORE
 * the webServer daemon boots (Playwright starts webServers before the
 * globalSetup hook, so this cannot be a globalSetup).
 */
export default function prepareWorld() {
  fs.rmSync(tmpDir, { recursive: true, force: true });
  fs.mkdirSync(path.join(tmpDir, "data"), { recursive: true });

  // Fixture repository.
  const repo = path.join(tmpDir, "fixture-repo");
  fs.mkdirSync(repo, { recursive: true });
  git(repo, "init", "-b", "main");
  git(repo, "config", "user.email", "e2e@example.com");
  git(repo, "config", "user.name", "E2E");
  fs.writeFileSync(path.join(repo, "README.md"), "fixture\n");
  fs.mkdirSync(path.join(repo, ".openade"), { recursive: true });
  fs.writeFileSync(
    path.join(repo, ".openade/rules.md"),
    "Always run the tests.\n",
  );
  git(repo, "add", ".");
  git(repo, "commit", "-m", "init");

  // Harness shims: enough interactivity to exercise the PTY loop.
  const bin = path.join(tmpDir, "bin");
  fs.mkdirSync(bin, { recursive: true });
  for (const name of ["claude", "codex", "gemini"]) {
    const shim = path.join(bin, name);
    fs.writeFileSync(
      shim,
      `#!/bin/sh\n` +
        `echo "${name}-shim started in $(basename "$(pwd)")"\n` +
        `echo "args: $@"\n` +
        `while read line; do\n` +
        `  echo "got:$line"\n` +
        `  [ "$line" = "exit" ] && exit 0\n` +
        `done\n`,
    );
    fs.chmodSync(shim, 0o755);
  }

  // gh shim: stands in for the user's authenticated GitHub CLI, serving the
  // repo:acme/checkout-service fixture (the daemon auto-detects it on PATH).
  const gh = path.join(bin, "gh");
  fs.writeFileSync(
    gh,
    `#!/bin/sh
case "$*" in
  "repo view acme/checkout-service --json"*)
    printf '{"name":"checkout-service","owner":{"login":"acme"},"description":"Checkout flow service for the acme shop.","url":"https://github.com/acme/checkout-service","homepageUrl":"","repositoryTopics":[{"name":"checkout"}],"primaryLanguage":{"name":"Go"},"isArchived":false,"defaultBranchRef":{"name":"main"}}'
    ;;
  "api repos/acme/checkout-service/contents/.github/CODEOWNERS"*)
    printf '* @acme/checkout-team\\n'
    ;;
  "api repos/acme/checkout-service/contents/README.md"*)
    printf '# Checkout Service\\nOwns the checkout flow.\\n'
    ;;
  "search repos"*)
    printf '[{"fullName":"acme/checkout-service","description":"Checkout flow service.","url":"https://github.com/acme/checkout-service"}]'
    ;;
  *)
    echo "gh: Not Found (HTTP 404)" >&2
    exit 1
    ;;
esac
`,
  );
  fs.chmodSync(gh, 0o755);

  process.env.OPENADE_E2E_REPO = repo;
}

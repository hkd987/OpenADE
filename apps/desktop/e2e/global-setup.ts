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
  // A second, unconfigured daemon world for the first-run onboarding flow
  // (the main daemon has memory env vars set, which counts as onboarded).
  fs.mkdirSync(path.join(tmpDir, "data-onboarding"), { recursive: true });
  // Fresh state for the multiplayer workspace server.
  fs.mkdirSync(path.join(tmpDir, "server-data"), { recursive: true });

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
  fs.mkdirSync(path.join(repo, ".openade/skills"), { recursive: true });
  fs.writeFileSync(
    path.join(repo, ".openade/skills/release.md"),
    "# Release\nCut and publish a release.\n",
  );
  git(repo, "add", ".");
  git(repo, "commit", "-m", "init");
  // A GitHub origin remote: sessions launched without an entity auto-ground
  // in repo:acme/checkout-service (served by the gh shim below).
  git(
    repo,
    "remote",
    "add",
    "origin",
    "https://github.com/acme/checkout-service.git",
  );

  // Harness shims: enough interactivity to exercise the PTY loop.
  const bin = path.join(tmpDir, "bin");
  fs.mkdirSync(bin, { recursive: true });
  for (const name of ["claude", "codex", "gemini", "copilot", "opencode"]) {
    const shim = path.join(bin, name);
    fs.writeFileSync(
      shim,
        `#!/bin/sh\n` +
        `echo "${name}-shim started in $(basename "$(pwd)")"\n` +
        `echo "args: $@"\n` +
        `trap 'exit 0' TERM INT\n` +
        `while :; do\n` +
        `  if IFS= read -r line; then\n` +
        `    echo "got:$line"\n` +
        `    [ "$line" = "exit" ] && exit 0\n` +
        `  else\n` +
        `    sleep 0.1\n` +
        `  fi\n` +
        `done\n`,
    );
    fs.chmodSync(shim, 0o755);
  }

  // Shared team memory repo state: the gh shim implements the GitHub
  // contents API for acme/team-memory against this directory, so the
  // daemon's direct-to-main knowledge pushes are observable on disk.
  const teamMemory = path.join(tmpDir, "team-memory");
  fs.mkdirSync(teamMemory, { recursive: true });

  // gh shim: stands in for the user's authenticated GitHub CLI, serving the
  // repo:acme/checkout-service fixture (the daemon auto-detects it on PATH).
  const gh = path.join(bin, "gh");
  fs.writeFileSync(
    gh,
    `#!/bin/sh
STATE="${teamMemory}"
case "$*" in
  "auth status"*)
    exit 0
    ;;
  "pr list"*)
    printf '[{"number":7,"title":"Add retries","url":"https://github.com/acme/checkout-service/pull/7","headRefName":"retries","isDraft":false}]'
    ;;
  "api -X PUT repos/acme/team-memory/contents/"*)
    file="\${4#repos/acme/team-memory/contents/}"
    if [ -e "$STATE/$file" ]; then
      case "$*" in
        *" sha="*) ;;
        *) echo 'gh: Invalid request. sha was not supplied. (HTTP 422)' >&2; exit 1 ;;
      esac
    fi
    mkdir -p "$STATE/$(dirname "$file")"
    printf '%s' "\${8#content=}" | base64 -d > "$STATE/$file"
    printf '{"content":{"path":"%s"}}' "$file"
    ;;
  "api repos/acme/team-memory/contents/"*" -H Accept: application/vnd.github.raw")
    file="\${2#repos/acme/team-memory/contents/}"
    if [ -e "$STATE/$file" ]; then cat "$STATE/$file"; else echo 'gh: Not Found (HTTP 404)' >&2; exit 1; fi
    ;;
  "api repos/acme/team-memory/contents/"*)
    file="\${2#repos/acme/team-memory/contents/}"
    if [ -e "$STATE/$file" ]; then
      printf '{"sha":"%s"}' "$(cksum "$STATE/$file" | cut -d' ' -f1)"
    else
      echo 'gh: Not Found (HTTP 404)' >&2; exit 1
    fi
    ;;
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

# OpenADE

**An open, vendor-neutral agentic development environment.**

OpenADE orchestrates parallel AI coding agent sessions — **Claude Code,
Codex CLI, Gemini CLI, Copilot CLI** — in isolated Git worktrees from a
single control surface,
and grounds every session in organizational memory via **MCP**: a
**Backstage** software catalog (ownership, dependencies, APIs, ADRs,
TechDocs) and/or **GitHub repositories** through your locally-authenticated
`gh` CLI (repo metadata, CODEOWNERS ownership, README/docs). Session
knowledge flows back as reviewed documentation, so every session makes the
next one smarter.

The context layer is open and pluggable — the inverse of commercial,
closed-context agent environments. Bring your own catalog; no accounts, no
telemetry, local-first.

![OpenADE session grid](docs/img/manual-e2e-ui.png)

## What it does

- **Parallel sessions, zero collisions** — every task gets its own Git
  worktree on its own branch, with a dirty-state guard on cleanup — or run
  a session directly in the main checkout when you want the tree you
  already have open (the main checkout is never cleaned up).
- **Persistent PTY sessions** — harnesses run in daemon-owned terminals that
  survive the window closing; reattach with full scrollback. Session states
  (`running` / `needs-input` / `completed` / `failed`) surface in a grid,
  including prompt detection for "the agent is waiting on you".
- **Context-grounded sessions, zero config** — memory works without
  thinking about it: launch a session with no entity named and it grounds
  itself in the repo's own GitHub `origin` remote. The agent starts with a
  budgeted context bundle (owner, dependencies, APIs, docs, prior session
  outcomes) injected into its rules file, plus the `catalog` MCP server
  registered for on-demand retrieval (`get_entity`, `get_owner`,
  `get_dependencies`, `get_apis_for_entity`, `search_catalog`,
  `get_techdocs_page`). Naming an entity is only for overriding: two memory
  sources, routed by ref — `component:ns/name` → Backstage;
  `repo:owner/name` → GitHub via your local `gh` CLI (OpenADE never touches
  GitHub credentials).
- **Knowledge loop** — one click summarizes a session (transcript + diff)
  into a markdown artifact committed on an `openade/knowledge-*` review
  branch under `docs/`; once merged it feeds the context bundle of the next
  session on that entity.
- **Shared team memory** — commit a one-line `.openade/memory-repo` file
  (`acme/team-memory`) naming a repo the whole team has write access to,
  and every member's published artifacts are *also* pushed straight to its
  default branch (`sessions/<slug>.md` + a living `index.md`) through
  their own local `gh` CLI — configured once for the team, zero setup per
  person (`OPENADE_MEMORY_REPO=owner/name` works too, as a daemon-wide
  default). Entries there matching a session's entity flow back into the
  next context bundle — what any teammate learned, everyone's next session
  knows. GitHub memory needs the [GitHub CLI](https://cli.github.com)
  installed and authenticated (`gh auth login`); the daemon tells you at
  startup — with the fix — if `gh` is missing or logged out.
- **Cross-harness handoff** — move a task Claude → Gemini (any direction) in
  place: same worktree and branch, rules re-materialized, a written handoff
  summary, and the new harness prompted to pick up where the old one left
  off.
- **One rules source** — `.openade/rules.md` materializes to `CLAUDE.md` /
  `AGENTS.md` / `GEMINI.md` so behavior doesn't change when you switch
  models (hand-written files are never clobbered).
- **One control surface** — Sessions and Projects views (per-repo state
  counts, last activity, open PRs via your `gh`, and a goal box: describe
  an outcome, a session launches), a Ctrl/⌘K command palette, a live
  daemon-health dot, collapsible per-project groups with one-click launch,
  cards/compact layouts, per-session Files (click-to-view), Rules, and
  Skills tabs, and a settings dialog that applies changes immediately.

Launching a session grounded in a GitHub repo memory entity:

![New session form with a GitHub memory entity](docs/img/new-session-form.png)

Publishing its knowledge — review branch locally, pushed to the shared team
memory repo immediately:

![Knowledge artifact pushed to shared team memory](docs/img/artifact-shared-memory.png)

## Install

**From a release** (Linux x86_64/aarch64, macOS Intel/Apple Silicon):
download the tarball for your platform from
[GitHub Releases](https://github.com/hkd987/OpenADE/releases), verify the
checksum, and put `openade-daemon` and `catalog-mcp` on your PATH.

**From source** (Rust 1.80+):

```sh
cargo install --path crates/openade-daemon --path crates/catalog-mcp
```

The harness CLIs themselves are bring-your-own: install and authenticate
`claude`, `codex`, `gemini`, and/or `copilot` through each vendor's normal
flow. OpenADE never touches their credentials. (Adapter flag mappings are
verified against current CLI releases via the
[Phase 0 spike](docs/phase-0-spike.md) checklist — these tools change
monthly.)

## Quickstart

```sh
# 0. For GitHub memory (repo: entities and the shared team memory repo),
#    install the GitHub CLI (https://cli.github.com) and authenticate:
#      gh auth login && gh auth status
#    OpenADE shells out to your gh — it never stores GitHub credentials.
#    Missing or logged-out gh is reported in the daemon log with the fix.

# 1. Start the daemon (localhost:7433). Memory sources are optional and
#    combine: Backstage via env; GitHub automatically whenever the gh CLI
#    is installed and authenticated (gh auth login).
export BACKSTAGE_BASE_URL=https://backstage.example.com   # optional
export BACKSTAGE_TOKEN=...                                # optional
openade-daemon

# (team, once) commit the shared memory repo into the project —
# every member's OpenADE picks it up with zero personal setup:
echo "acme/team-memory" > /path/to/your/repo/.openade/memory-repo
#   (OPENADE_MEMORY_REPO=owner/name also works as a daemon-wide default)

# 2. Launch a session (or use the UI below). No entity_ref needed —
#    the session grounds itself in the repo's GitHub origin remote.
curl -s -X POST localhost:7433/sessions -H 'content-type: application/json' -d '{
  "title": "add retries to the payments client",
  "harness": "claude-code",
  "repo_root": "/path/to/your/repo",
  "prompt": "Add retries with exponential backoff to the payments client."
}'
# Naming one overrides the auto-detection:
#   "entity_ref": "component:default/payments-api"   (Backstage)
#   "entity_ref": "repo:acme/payments"               (GitHub)

# 3. Run the UI (dev mode; Node 20+)
cd apps/desktop && npm install && npm run dev
```

**First run:** the app opens with a 30-second onboarding that checks your
GitHub CLI (and tells you exactly how to fix it if it's missing or signed
out) and collects the optional settings — Backstage URL/token and the
shared team memory repo. Saved settings live in the daemon's
`config.json` (under `OPENADE_DATA_DIR`) and apply immediately, no
restart; environment variables always take precedence over them.
Everything is skippable because GitHub memory is zero-config.

![First-run onboarding](docs/img/onboarding.png)

Useful daemon endpoints: `GET /sessions`, `GET /sessions/{id}/scrollback`,
`POST /sessions/{id}/input`, `GET /sessions/{id}/diff`, `.../files`,
`POST /sessions/{id}/artifact`, `POST /sessions/{id}/handoff`,
`DELETE /sessions/{id}`, `DELETE /sessions/{id}/worktree`, `GET /projects`,
`GET`/`PUT /config` (settings + memory/gh status, used by onboarding).

Environment knobs: `OPENADE_DAEMON_PORT` (default 7433), `OPENADE_DATA_DIR`
(default `~/.openade` — transcripts, session index, worktrees, and the
onboarding-written `config.json`),
`BACKSTAGE_BASE_URL` / `BACKSTAGE_TOKEN` (Backstage memory source),
`OPENADE_GH_BIN` (gh binary override; defaults to the `gh` CLI on PATH),
`OPENADE_GITHUB_MEMORY=0` (kill switch for everything gh-backed: the GitHub
memory source, the shared memory repo, and status probes),
`OPENADE_MEMORY_REPO=owner/name` (daemon-wide
shared team memory repo; a repository's committed `.openade/memory-repo`
file takes precedence — artifacts are pushed directly to its default
branch, so everyone on it needs write access),
`VITE_OPENADE_DAEMON_URL` (UI → daemon).

## Architecture

```
┌─────────────────────────────────────────────────────┐
│ Desktop app (Tauri 2: TS/React UI, xterm.js)        │  apps/desktop
└──────────────┬──────────────────────────────────────┘
               │ localhost HTTP
┌──────────────▼──────────────────────────────────────┐
│ Session daemon (Rust)                               │  crates/openade-daemon
│  • PTY host — sessions survive window close         │
│  • Git worktree isolation per task                  │
│  • Harness adapters (claude/codex/gemini/copilot)   │
│  • Context bundles + knowledge artifacts            │
│  • Transcript recorder (JSONL + SQLite index)       │
└───────┬─────────────────────────────┬───────────────┘
        │ spawns + MCP config         │
┌───────▼───────────┐        ┌────────▼───────────────┐
│ Harness CLIs      │  MCP   │ catalog-mcp (Rust)     │  crates/catalog-mcp
│ (user-installed,  │◄──────►│  CatalogProvider trait │
│  user-authed)     │        │  └ MemoryRouter        │
└───────────────────┘        └───┬──────────────┬─────┘
                     REST (read) │              │ local gh CLI (read)
                        ┌────────▼─────────┐ ┌──▼──────────────────┐
                        │ Your Backstage   │ │ GitHub repos        │
                        │ (catalog, docs)  │ │ (CODEOWNERS, docs)  │
                        └──────────────────┘ └─────────────────────┘
```

Shared domain types live in `crates/openade-core`. Memory sources implement
the `CatalogProvider` trait and are composed by kind (`repo:` → GitHub,
everything else → Backstage) — neither backend is a hard dependency.

## Documentation

| Doc | What |
|---|---|
| [PRD](docs/PRD.md) | Product requirements — the why and the roadmap |
| [ADR-001](docs/adr/ADR-001-desktop-shell.md) | Desktop shell decision (Tauri vs Electron vs TUI) |
| [catalog-mcp tools](docs/catalog-mcp-tools.md) | MCP tool schemas, auth, design rules |
| [Phase 0 spike](docs/phase-0-spike.md) | Real-CLI verification plan (the gate to beta) |
| [Testing & coverage](docs/testing.md) | Test strategy, 100% line coverage methodology |
| [Manual e2e record](docs/manual-e2e.md) | By-hand verification of the assembled system |
| [Desktop app](apps/desktop/README.md) | UI development and the Tauri shell |
| [Contributing](CONTRIBUTING.md) | Ground rules and dev setup |

## Testing

```sh
cargo test                                        # 171 Rust tests (real git, real PTYs, mock Backstage, fake gh, fault injection)
cd apps/desktop && npm test                       # UI unit tests (vitest)
cd apps/desktop && npm run e2e                    # 14 Playwright flows: real daemon + mock Backstage + gh shim + real Chromium
```

Product-code line coverage is 100% on both the Rust workspace and the UI —
methodology and the e2e flow matrix are in [docs/testing.md](docs/testing.md);
the by-hand verification (including the native Tauri shell under WebKitGTK)
is in [docs/manual-e2e.md](docs/manual-e2e.md).

CI runs fmt, clippy (`-D warnings`), all three test layers, and a
Tauri-shell compile check. Releases are one button: Actions → Release → "Run workflow" on main reads the version from Cargo.toml, runs the verification gate, builds the binaries, and publishes the tagged GitHub Release (tag pushes also work; see
`.github/workflows/release.yml`).

## Principles

- **No model access in our code.** You authenticate each harness through its
  own CLI; OpenADE never proxies API keys.
- **Local-first.** Transcripts and session state stay on your machine unless
  you explicitly publish a knowledge artifact (as a reviewable Git branch).
- **Two swappable layers.** Harnesses behind adapters; catalogs behind
  `CatalogProvider`. No lock-in on either side.

## License

[Apache-2.0](LICENSE)

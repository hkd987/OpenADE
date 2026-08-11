# OpenADE

**An open, vendor-neutral agentic development environment.**

> ⚠️ Status: **pre-alpha scaffold**. The core crates build and are tested; the
> product is not usable end-to-end yet. See [docs/PRD.md](docs/PRD.md) for
> where this is going and [docs/phase-0-spike.md](docs/phase-0-spike.md) for
> what's being validated next. "OpenADE" is a working codename (PRD Q4).

OpenADE orchestrates parallel AI coding agent sessions — **Claude Code, Codex
CLI, Gemini CLI** — in isolated Git worktrees from a single control surface,
and grounds every session in your organization's context from a **Backstage**
software catalog via **MCP**: ownership, dependencies, APIs, ADRs, TechDocs.
Session knowledge flows back as reviewed documentation, so every session makes
the next one smarter.

The context layer is open and pluggable — the inverse of commercial,
closed-context agent environments. Bring your own catalog; no accounts, no
telemetry, local-first.

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
│  • Harness adapters (claude / codex / gemini)       │
│  • Transcript recorder (JSONL + SQLite index)       │
└───────┬─────────────────────────────┬───────────────┘
        │ spawns + MCP config         │
┌───────▼───────────┐        ┌────────▼───────────────┐
│ Harness CLIs      │  MCP   │ catalog-mcp (Rust)     │  crates/catalog-mcp
│ (user-installed,  │◄──────►│  CatalogProvider trait │
│  user-authed)     │        │  └ BackstageProvider   │
└───────────────────┘        └────────┬───────────────┘
                                      │ REST (read-only)
                             ┌────────▼───────────────┐
                             │ Your Backstage         │
                             └────────────────────────┘
```

Shared domain types (sessions, context bundles, rules normalization) live in
`crates/openade-core`.

## What works today

- **Worktree isolation** (PRD R2): task branches + worktrees with a
  dirty-state guard; 10 parallel sessions on one repo, zero collisions
  (tested).
- **PTY session host** (R1 groundwork): daemon-owned PTYs with capped
  scrollback, exit tracking, input injection (tested against real shells).
- **Rules normalization** (R4): one canonical `.openade/rules.md` materialized
  to `CLAUDE.md` / `AGENTS.md` / `GEMINI.md`, never clobbering hand-written
  files (tested).
- **Harness adapters** (R4): launch/resume/MCP-registration mapping per CLI —
  flags pending real-CLI verification in the Phase 0 spike.
- **Transcript capture** (R6 groundwork): JSONL event logs + SQLite index,
  queryable by catalog entity (tested).
- **catalog-mcp** (R5): MCP stdio server exposing `get_entity`, `get_owner`,
  `get_dependencies`, `get_apis_for_entity`, `search_catalog`,
  `get_techdocs_page` against Backstage, behind a swappable `CatalogProvider`
  trait; context bundle builder with a token budget (tested against mock
  Backstage).
- **Desktop UI scaffold**: session grid + xterm terminal attached to the
  daemon API.

## Quickstart (development)

Prereqs: Rust (1.80+), git. Node 20+ for the UI.

```sh
# Build + test everything (daemon, catalog-mcp, core)
cargo test

# Run the session daemon (localhost:7433)
cargo run -p openade-daemon

# Launch a session in a fresh worktree (harness CLIs are BYO; this example
# uses a plain shell via command_override)
curl -s -X POST localhost:7433/sessions -H 'content-type: application/json' -d '{
  "title": "try openade",
  "harness": "claude-code",
  "repo_root": "/path/to/some/git/repo",
  "command_override": {"program": "sh", "args": []}
}'

# Run the catalog MCP server against your Backstage
BACKSTAGE_BASE_URL=https://backstage.example.com \
BACKSTAGE_TOKEN=... \
cargo run -p catalog-mcp

# UI (see apps/desktop/README.md for the Tauri shell)
cd apps/desktop && npm install && npm run dev
```

## Repository layout

| Path | What |
|---|---|
| `crates/openade-core` | Shared types: sessions, harnesses, context bundles, rules |
| `crates/openade-daemon` | Session daemon: PTY host, worktrees, adapters, transcripts, HTTP API |
| `crates/catalog-mcp` | MCP context server + `CatalogProvider` / Backstage backend |
| `apps/desktop` | Tauri 2 + React UI (workspace-excluded shell crate) |
| `docs/` | [PRD](docs/PRD.md) · [ADR-001 shell choice](docs/adr/ADR-001-desktop-shell.md) · [Phase 0 spike](docs/phase-0-spike.md) · [catalog-mcp tools](docs/catalog-mcp-tools.md) |

## Principles

- **No model access in our code.** You authenticate each harness through its
  own CLI; OpenADE never proxies API keys.
- **Local-first.** Transcripts and session state stay on your machine unless
  you explicitly publish a knowledge artifact (as a reviewable PR).
- **Two swappable layers.** Harnesses behind adapters; catalogs behind
  `CatalogProvider`. No lock-in on either side.

## License

[Apache-2.0](LICENSE)

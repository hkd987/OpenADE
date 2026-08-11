# Phase 0 spike — ticket breakdown

**Goal (PRD §8):** go/no-go on the harness adapter layer in 2 weeks.
**Exit criteria:** every ⚠️ assumption in the adapter table (PRD §7.4,
`crates/openade-daemon/src/adapters.rs`) is verified against current CLI
versions, and PRD Q1 (real resume vs. PTY-only persistence) has an answer per
harness.

The scaffold in this repo already proves the harness-independent half (PTY
persistence with scrollback, worktree isolation, transcript capture — all
under `cargo test`). The spike is about the parts that need the real,
authenticated CLIs, which cannot run in CI.

## Tickets

### S0.1 — Version pinning & test matrix (½ day)
Record exact versions of `claude`, `codex`, `gemini` CLIs under test; create a
scratch repo + a throwaway Backstage demo instance (`npx @backstage/create-app`
or the public demo) for the whole spike. Every later ticket reports against
these versions.

### S0.2 — PTY behavior per harness (2 days)
Launch each CLI through `openade-daemon`'s PTY host (use `command_override` to
baseline against plain `sh` first).
- Does the CLI behave correctly under a PTY it doesn't own (colors, raw mode,
  resize via `PtySession::resize`)?
- Does scrollback capture stay coherent under heavy TUI redraw (Claude Code
  and Gemini repaint aggressively — measure buffer churn vs. the 2MB cap)?
- **needs-input detection:** find a reliable signal per CLI (prompt regex?
  process state? OSC sequences?) for the `NeedsInput` session state — the grid
  is lying without it. Deliverable: detection strategy memo per harness.

### S0.3 — Resume semantics (PRD Q1, blocking) (3 days)
Per harness, answer: after daemon restart (not window restart — the PTY layer
already covers that), can we resume the *agent's* session?
- Claude Code: verify `claude --resume <id>` and where session ids can be
  scraped (`~/.claude/projects/...`). Confirm `--continue` fallback.
- Codex CLI: verify `codex resume <id>` exists in the pinned version and what
  state it restores (conversation? sandbox settings?).
- Gemini CLI: verify checkpoint/`--resume` behavior and whether checkpoints
  are automatic or opt-in.
- Deliverable: fill in `resume_command()` per adapter with tested flags, or
  document "PTY-layer persistence only" for that harness and adjust R1
  acceptance messaging.

### S0.4 — Rules file mapping (1 day)
Materialize one canonical `.openade/rules.md` (already implemented,
`openade_core::rules`) and verify each CLI actually loads its file from a
worktree root: `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`.
- Check precedence vs. user-level rules files and vs. nested dirs.
- Check whether any harness chokes on the generated-marker HTML comment.

### S0.5 — MCP registration (2 days)
Register a stub MCP server (use `catalog-mcp` against the demo Backstage) with
each CLI, per the mechanisms in `adapters.rs`:
- Claude Code: project `.mcp.json` in the worktree — is it auto-trusted or
  does it prompt? Does `claude mcp add --scope project` write the same file?
- Codex: `~/.codex/config.toml` `[mcp_servers.*]` — confirm schema and whether
  a project-scoped alternative landed in the pinned version.
- Gemini: `.gemini/settings.json` `mcpServers` — confirm project-scope loading
  and trust prompts.
- Deliverable: per-harness "time to first successful `get_entity` call" and
  any trust/consent UX we must document for users.

### S0.6 — Backstage read API spike (2 days)
Run `catalog-mcp` (this repo) against the demo Backstage instance instead of
wiremock:
- Verify entity fetch, `by-query` full-text search, TechDocs static content
  paths against a real instance (auth via static token).
- Measure: context bundle build latency for an entity with ~20 relations;
  token size of `ContextBundle::to_markdown()` vs. the 4K budget.
- Note divergences between real API responses and our serde shapes
  (`provider.rs`) — extra fields are tolerated, missing ones are the risk.

### S0.7 — Cross-harness same-brief run (1 day, integration)
The R4 acceptance dry run: one task brief ("add a health endpoint to this
service"), executed on all three harnesses from the daemon, each in its own
worktree, MCP registered, rules materialized. Record manual steps that were
still needed per harness — that list is Phase 1's backlog.

## Go/no-go rubric

- **Go:** S0.2 PTY behavior is solid for ≥3 harnesses AND MCP registration
  works for ≥3 (resume can degrade to PTY-only for at most one harness —
  that's a caveat, not a blocker, since the daemon PTY layer already gives
  window-level persistence).
- **No-go / reshape:** any CLI is unusable under a foreign PTY (would force a
  headless/exec-mode adapter design instead), or MCP registration requires
  per-session user interaction we can't automate (would force bundling
  context via rules files instead of MCP for that harness).

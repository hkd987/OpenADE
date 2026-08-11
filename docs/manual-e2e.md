# Manual end-to-end verification

**Date:** 2026-08-11 · **Build:** release binaries (`cargo build --release`)
· **Environment:** Linux container; harness CLIs stood in by interactive
shell shims on PATH; Backstage stood in by a local mock serving the catalog
by-name/by-query and TechDocs REST endpoints.

Everything below was executed by hand against the assembled system (not the
test suite): `openade-daemon` release binary + mock Backstage + built UI in
real Chromium.

![OpenADE UI during the manual pass](img/manual-e2e-ui.png)

*The grid above (captured during this pass) shows the three states exercised:
a Codex session waiting on input, a killed Gemini session, and the completed
Claude session — all on catalog entity `component:default/payments-api`.*

## Checklist and observed results

| # | Step | Result |
|---|---|---|
| 1 | `GET /health` on the release daemon | `{"status":"ok","version":"0.1.0"}` |
| 2 | Launch entity session (`component:default/payments-api`, Claude, prompt) | `running`, task branch `openade/add-idempotency-keys-*`, isolated worktree |
| 3 | Context bundle injection | `CLAUDE.md` carries "System context: Payments API" incl. owner "Payments Team" from mock Backstage; `.openade/context.{md,json}` written |
| 4 | Catalog MCP auto-registration | `.mcp.json` in worktree registers `catalog` → `catalog-mcp` (stdio) |
| 5 | Scrollback | Shim banner + passed-through prompt visible via `GET .../scrollback` |
| 6 | Needs-input detection | After the PTY echoed a `(y/n)` prompt and quiesced, `GET /sessions/{id}` flipped to `needs-input` |
| 7 | Diff view | Edit to `README.md` in the worktree appears as `+idempotency keys added` |
| 8 | File browser | Lists tracked + generated files, gitignore respected |
| 9 | Projects | Repo listed from the session index |
| 10 | Knowledge artifact | Committed to branch `openade/knowledge-2026-08-11-add-idempotency-keys-*` at `docs/openade/sessions/…md`; primary checkout untouched (`git status` clean) |
| 11 | Handoff Claude → Gemini | Same worktree/branch, `GEMINI.md` gets the context bundle, `.openade/handoff.md` written, new PTY got the takeover prompt |
| 12 | Knowledge loop | A follow-up session on the same entity received "Prior sessions on this entity" with the earlier outcomes in its bundle |
| 13 | Kill + cleanup | Kill → `failed`; worktree delete → `409` while dirty, `204` with `?force=true`, directory removed |
| 14 | `catalog-mcp` binary over stdio vs. mock Backstage | `initialize` handshake; `get_owner` → "Payments Team"; `search_catalog` → payments-api, ledger; `get_techdocs_page` → ADR content |
| 15 | UI in Chromium (production build via `vite preview`) | Grid renders all sessions/states, terminal attaches and streams, tabs and action buttons work (screenshot above) |

## Not verifiable in this environment

- The real `claude` / `codex` / `gemini` CLIs (no vendor credentials in the
  container) — adapter flags remain gated on the
  [Phase 0 spike](phase-0-spike.md) run on a developer machine.
- The Tauri native shell (no WebKitGTK in the container) — the UI was
  verified in Chromium instead; the shell is compile-checked in CI.
- A production Backstage instance — the REST surface was mocked
  byte-for-byte per the API docs; `wiremock` tests cover the same paths.

## Reproducing this pass

The e2e suite automates the same story: `cd apps/desktop && npm run e2e`.
To do it by hand, follow the commands in this file's history or the
quickstart in the [README](../README.md), exporting `BACKSTAGE_BASE_URL`
(and optionally `BACKSTAGE_TOKEN`) before starting the daemon.

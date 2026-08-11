# Testing & coverage

Three layers, all wired into CI (`.github/workflows/ci.yml`):

| Layer | What runs | Command |
|---|---|---|
| Rust unit/integration | 119 tests across the workspace — real git repos (worktrees, artifact branches), real PTYs (spawned shells), real HTTP routers (tower `oneshot`), mock Backstage (`wiremock`), and real fault injection (corrupt SQLite, failing git, truncated HTTP bodies) | `cargo test` |
| UI unit | vitest + testing-library: API client and every component (App, session card, launch form, session detail, terminal) | `cd apps/desktop && npm test` |
| End-to-end | 10 Playwright tests drive real Chromium against the real daemon (`cargo run`), a mock Backstage, and the Vite dev server; harness CLIs are shims created per run | `cd apps/desktop && npm run e2e` |

## End-to-end flow matrix

Every user-facing flow has an e2e test (`apps/desktop/e2e/session-flows.spec.ts`):

| Flow | Test |
|---|---|
| Daemon health + empty grid | `grid starts empty and the daemon is reachable` |
| Launch from form → live PTY terminal, prompt pass-through | `launching a session from the form attaches a live terminal` |
| Diff + file browser views | `diff and file views reflect worktree changes` |
| Terminal input round-trip | `terminal input reaches the harness process` |
| Knowledge artifact → review branch | `knowledge artifact lands on a review branch` |
| Cross-harness handoff, same worktree | `handoff moves the task to another harness…` |
| Window close/reopen reattach (R1 acceptance) | `reopening the app reattaches sessions with full scrollback` |
| Entity launch → context bundle + MCP registration + entity filter | `entity-launched sessions carry catalog context` |
| Needs-input state in the grid | `a session waiting on input shows needs-input in the grid` |
| Kill → failed state | `killing a session marks it failed in the grid` |

Endpoint-level flows without UI affordances (worktree cleanup with the dirty
guard, error statuses, entity-filtered listings) are covered end-to-end at
the HTTP layer inside the Rust suite (`server_tests.rs`) and in the
[manual pass](manual-e2e.md).

## Principles

- **No mocking of git or PTYs.** Worktree and terminal behavior is only
  trustworthy against the real things; tests create throwaway repos and spawn
  real shells.
- **The network is mocked at the HTTP boundary** (`wiremock` serving canned
  Backstage API responses) — never inside our own code.
- **The e2e suite runs the shipped binary path**: `cargo run -p
  openade-daemon` + browser, with `claude`/`codex`/`gemini` stand-in shims on
  PATH, so launch → PTY → attach → diff → artifact → handoff → kill is
  exercised exactly as an operator experiences it.

## Coverage

**Rust product code: 100% line coverage** (all 1,887 instrumented lines
across all 15 product source files execute under `cargo test`), measured
with `cargo llvm-cov` in lcov line accounting:

```
cargo llvm-cov --workspace \
  --ignore-filename-regex '(main\.rs|testutil\.rs|_tests\.rs)' \
  --lcov --output-path cov.lcov
```

**UI product code: 100% line and function coverage** (vitest v8, all
components and the API client; `main.tsx` DOM bootstrap excluded — the
Playwright suite loads the real bundle).

Methodology, stated plainly:

- Test modules live in sibling `*_tests.rs` files so the measurement covers
  product code, not the tests themselves. Binary `main.rs` entrypoints
  (thin wiring) and the `testutil` mock are excluded and exercised by the
  e2e suite and the manual pass.
- Error paths are covered by *real* fault injection, not filesystem mocks:
  corrupt SQLite files, dropped index tables, colliding files/directories
  committed into fixture repos, truncated HTTP bodies from a raw socket,
  unreachable ports, a marker-scoped failing `git` wrapper, and PTY spawns
  of missing binaries.
- The remaining sub-line *region* gaps (llvm's stricter accounting) are the
  never-taken halves of `?`/`&&` operators on executed lines — e.g. the
  success half of an error-exit that fired, or vice versa.

Manual verification of the assembled system is recorded in
[manual-e2e.md](manual-e2e.md).

## Running everything locally

```sh
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test
cd apps/desktop && npm ci && npm test && npm run build && npm run e2e
```

The e2e run compiles and boots the daemon itself; nothing else needs to be
running. After heavy local builds, `cargo clean` reclaims the `target/`
directory (multi-GB).

# Testing & coverage

Three layers, all wired into CI (`.github/workflows/ci.yml`):

| Layer | What runs | Command |
|---|---|---|
| Rust unit/integration | 101 tests across the workspace — real git repos (worktrees, artifact branches), real PTYs (spawned shells), real HTTP routers (tower `oneshot`), mock Backstage (`wiremock`) | `cargo test` |
| UI unit | vitest + testing-library: API client, session card, launch form | `cd apps/desktop && npm test` |
| End-to-end | Playwright drives real Chromium against the real daemon (`cargo run`) and Vite dev server; harness CLIs are shims created per run | `cd apps/desktop && npm run e2e` |

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

Measured with `cargo llvm-cov` (line coverage, binary entrypoints and test
utilities excluded — `main.rs` files are thin wiring exercised by the e2e
suite and the manual pass instead):

```
cargo llvm-cov --workspace --summary-only --ignore-filename-regex '(main\.rs|testutil\.rs)'
```

Snapshot at the time of writing (2026-08-11):

| Area | Line coverage |
|---|---|
| `openade-core` | 96.9% (context 100%, session 100%, harness 98%, rules 90%) |
| `openade-daemon` | 96.0% (server 97.8%, worktree 97.9%, artifact 99.4%, pty 96.0%, daemon 94.3%, transcript 96.2%) |
| `catalog-mcp` | 97.8% (provider 100%, mcp 99.0%, backstage 97.7%, bundle 95.6%) |
| **Workspace total** | **96.9%** |

The uncovered remainder is almost entirely I/O-failure branches (disk write
errors, poisoned locks, `tracing` warn arms) that would need fault injection
to reach; we prefer honest numbers over mocking the filesystem. UI unit
coverage (vitest v8) is ~89% lines on the unit-tested modules; `App`,
`SessionDetail`, and `TerminalView` are covered by the Playwright suite
end-to-end rather than by DOM unit tests.

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

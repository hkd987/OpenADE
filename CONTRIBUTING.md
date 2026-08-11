# Contributing to OpenADE

Thanks for your interest! OpenADE is pre-alpha; the fastest way to help is to
pick up items from [docs/phase-0-spike.md](docs/phase-0-spike.md) (real-CLI
verification) or open an issue describing the harness/catalog setup you want
supported.

## Development setup

```sh
cargo test                                   # Rust workspace (daemon, catalog-mcp, core)
cargo clippy --all-targets -- -D warnings    # lints (CI-enforced)
cargo fmt --all                              # formatting (CI-enforced)
cd apps/desktop && npm install && npm run build   # UI type-check + build
```

The Tauri shell crate (`apps/desktop/src-tauri`) is excluded from the Cargo
workspace and needs platform webview libraries — see
[apps/desktop/README.md](apps/desktop/README.md). You do not need it for
daemon or catalog work.

## Ground rules

- **Tests come with the change.** Worktree/PTY behavior is tested against real
  git repos and real shells; catalog behavior against mocked Backstage HTTP
  (`wiremock`). Follow those patterns — no mocking of git.
- **Keep the two seams clean.** Harness-specific behavior belongs in a
  `HarnessAdapter`; catalog-backend behavior belongs in a `CatalogProvider`.
  If a change leaks Backstage specifics past the provider trait or CLI
  specifics past the adapter, it will be asked to move.
- **CLI facts need receipts.** The agent CLIs change monthly. Changes to
  adapter flags/paths should state the CLI version they were verified
  against (see the Phase 0 spike doc for the checklist).
- **No telemetry, no credential handling.** Don't add code that phones home or
  touches harness credentials; both are hard product commitments (PRD §7.5).

## CI policy: always green

`main` is expected to pass CI at all times. To keep it that way:

- **Reproduce CI locally before pushing.** The toolchain is pinned in
  `rust-toolchain.toml`, so the exact clippy/rustc CI uses runs on your
  machine — `cargo fmt --all --check && cargo clippy --all-targets -- -D
  warnings && cargo test`, plus `npm test && npm run build && npm run e2e`
  in `apps/desktop`. If it's green locally it's green in CI; there is no
  version drift to surprise you.
- **A red run on GitHub is a stop-the-line event.** Pull the failing job's
  log, reproduce the failure locally, fix it at the root (not by loosening
  the check), and push the fix. Don't stack unrelated work on a red `main`.
- **Toolchain bumps are deliberate.** New stable clippy lints arrive by
  updating `rust-toolchain.toml` in its own commit, fixing whatever the new
  lints flag in that same commit.

## Pull requests

- Branch from `main`; keep PRs focused.
- CI must pass: fmt, clippy `-D warnings`, tests (unit + e2e), UI build.
- License: by contributing you agree your contributions are licensed under
  [Apache-2.0](LICENSE).

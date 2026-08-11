# ADR-001: Desktop shell — Tauri 2 over Electron or a TUI

**Status:** Accepted
**Date:** 2026-08-11
**Deciders:** OpenADE engineering
**Related:** PRD §7.2 (key technical decisions), PRD Non-Goal "Windows/Linux parity in v0.1"

## Context

OpenADE needs a control surface for many concurrent agent sessions: a session
grid, embedded terminals, diff views, and catalog context panels (PRD R3). The
shell choice constrains team skills, binary size, cross-platform reach, and —
critically — where session state is allowed to live.

Three candidates were considered:

1. **Tauri 2** — Rust core, system webview, TS/React frontend.
2. **Electron** — Node core, bundled Chromium, TS/React frontend.
3. **TUI** (Ratatui or similar) — pure-terminal UI, no webview at all.

A structural decision made independently of the shell, but which shapes the
evaluation: **all session state lives in a separate daemon process**
(`openade-daemon`), because sessions must survive window close/reopen (PRD R1
acceptance). The shell is a viewer. Any of the three candidates can render a
viewer; the question is which does it best for our constraints.

## Decision

**Tauri 2**, with the UI in TypeScript/React and terminals in xterm.js, talking
to `openade-daemon` over a localhost HTTP API (later: unix socket / WebSocket
streaming).

## Rationale

| Criterion | Tauri 2 | Electron | TUI |
|---|---|---|---|
| Team expertise fit | ✅ Rust core, same language as daemon/catalog-mcp | ⚠️ adds a Node main-process stack | ✅ Rust |
| Binary size / footprint | ✅ ~10MB-class, system webview | ❌ 100MB+ Chromium per app | ✅ tiny |
| Rich UI (grid, diffs, context panels) | ✅ full web platform | ✅ full web platform | ❌ severely constrained; diff views and entity cards suffer |
| Terminal embedding | ✅ xterm.js (proven with PTY backends) | ✅ xterm.js | ✅ native, but only one "pane" idiom |
| Cross-platform path (macOS → Linux → Windows) | ✅ first-class | ✅ first-class | ✅ trivially |
| Webview consistency risk | ⚠️ system webviews differ (WKWebView vs WebKitGTK) | ✅ bundled Chromium is uniform | n/a |
| Ecosystem maturity | ⚠️ younger than Electron | ✅ deepest | ✅ mature for what it does |

- **One language for the core.** Daemon, catalog-mcp, and shell share Rust
  types and tooling; contributors cross layers without switching stacks. With
  Electron we would maintain a Node IPC layer just for the shell.
- **Footprint is a product stance, not just engineering taste.** OpenADE's
  positioning against Xirp is local-first and lightweight; shipping a bundled
  Chromium undercuts that story, and Tauri's system-webview model is the main
  reason its binaries are an order of magnitude smaller.
- **The webview-consistency risk is contained** because our UI is a dashboard,
  not a pixel-perfect canvas: xterm.js, CSS grid, and fetch are well inside
  the compatibility envelope of WKWebView and WebKitGTK. We accept minor
  rendering variance.
- **A TUI cannot carry the product.** The session grid alone might work, but
  PRD R3 (diff view, file browser, rules panel) and the context bundle UX
  (entity cards, docs links) degrade badly in a terminal. A TUI also
  forfeits the P1 Backstage-plugin design language sharing. We may still ship
  a *thin* TUI attach client later (the daemon API makes that cheap), but not
  as the primary surface.

## Consequences

- **Linux builds need system packages** (WebKitGTK et al.); the `src-tauri`
  crate is excluded from the default Cargo workspace so `cargo build`/CI stay
  hermetic, and the shell builds behind an explicit dependency install
  (see `apps/desktop/README.md`). macOS needs only Xcode CLT.
- **The daemon API is the contract.** Everything the shell renders must be
  reachable over the localhost API — which is also what makes the P2
  remote/headless mode and a future TUI client feasible without rework.
- **Frontend dependencies (React, xterm.js, Vite) enter the repo**, with npm
  supply-chain hygiene to match (lockfile committed, CI builds from `npm ci`).
- Revisit trigger: if WebKitGTK divergence starts costing real engineering
  time on Linux (fast-follow target), the fallback is not Electron but
  Tauri's optional CEF backend or servo-based webviews as they mature.

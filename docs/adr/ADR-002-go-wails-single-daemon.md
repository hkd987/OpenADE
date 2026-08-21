# ADR-002: Go/Wails shell with one durable local daemon

**Status:** Accepted
**Date:** 2026-08-20
**Supersedes:** ADR-001 for the active desktop implementation

## Context

OpenADE must host multiple local coding-agent TUIs in isolated Git worktrees,
preserve them when the desktop window closes, and present session, ticket, and
pull-request state from one responsive shell. The existing Rust/Tauri prototype
proved the product model, but the active implementation is moving to Go 1.26+
with Wails 2.10.2 and CGO-enabled SQLite.

The architectural boundary matters more than the shell framework: agent
processes cannot belong to a window. A renderer crash or ordinary app quit must
not end a two-hour Claude Code or Codex session.

## Decision

Ship one `OpenADE` binary with two execution modes:

- Normal mode launches the Wails desktop shell.
- `OpenADE --daemon` launches the only stateful local service on
  `127.0.0.1:7433`.

On startup the shell probes the daemon. If no healthy process is listening, it
starts the same binary in daemon mode and releases the child process. Shell
shutdown deliberately leaves the daemon running.

The daemon exclusively owns:

- SQLite session and project indexes.
- PTY processes, input, resize, scrollback, and transcript files.
- Git worktree and branch creation.
- GitHub CLI queries, branch pushes, and draft pull-request creation.
- Jira CLI ticket lookup and ticket metadata attached to sessions.
- HTTP and WebSocket APIs consumed by the Wails webview.

The Wails shell owns presentation and ephemeral interaction state only.

## Session identity and policy

Every session has one repository, worktree, branch, agent CLI, transcript, and
optional ticket and pull request. A linked ticket becomes the branch prefix—for
example `ADE-101/fix-checkout-<id>`—so branch-policy checks can resolve the work
item without scraping chat. Draft PR titles also receive the ticket key when it
is absent.

Credentials remain outside OpenADE. Agent CLIs use their normal local login;
GitHub uses the authenticated `gh` CLI; Jira uses the configured local CLI.
SQLite stores references and session metadata, never tokens.

## Consequences

- The daemon is a small local reliability boundary and can later serve a CLI,
  menu-bar client, or remote-capable shell without moving process ownership.
- SQLite uses `mattn/go-sqlite3`, so release builds require `CGO_ENABLED=1`.
- PTY sessions survive the shell but are marked `interrupted` after a daemon or
  machine restart; native agent resume IDs can be layered on later.
- The localhost API must remain backward compatible enough for separately
  versioned shells, and every external write must remain an explicit user
  action.

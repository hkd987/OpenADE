# OpenADE

**A local-first agentic development environment for running multiple coding agents without mixing their work.**

OpenADE gives Claude Code, Codex CLI, GitHub Copilot CLI, and OpenCode a shared desktop control surface. Each task runs in its own Git worktree and branch, while one durable local daemon owns the PTYs, transcripts, queues, and SQLite session index.

The desktop window is only a client. Closing it does not stop active agents or project terminals.

> This branch contains the current Go + Wails desktop implementation. The older Rust/Tauri prototype remains in `crates/` as reference code, but it is not the runtime described below.

## Current capabilities

- **Isolated work per task** — every session gets a dedicated Git worktree and branch. Ticket keys are included in branch and pull-request naming when provided.
- **Durable sessions** — a single local daemon owns agent processes, PTYs, scrollback, transcripts, and SQLite state across desktop restarts.
- **Native chat** — Codex and Claude structured output streams into a Markdown conversation with collapsible activity, tool intent, code blocks, and message queueing.
- **Direct TUI** — attach to the real Codex or Claude terminal interface, including dynamic terminal resizing and provider resume behavior.
- **Independent terminals** — open multiple ordinary project shells without mixing shell output into the agent conversation.
- **Surface preferences** — choose Native chat or Direct TUI as the default. Opening an existing Codex or Claude session converts it to the preferred surface when necessary.
- **Project continuity** — scan a workspace root for Git repositories and discover resumable local Codex and Claude conversations.
- **Multi-agent overview** — browse running, waiting, completed, failed, and priority sessions across repositories.
- **Review workspace** — inspect changed files and unified diffs beside the conversation.
- **GitHub delivery** — list pull requests and push a session branch into a draft PR through the locally authenticated `gh` CLI.
- **Jira-linked work** — associate a Jira key and URL with a session and fetch ticket details through the local `jira` CLI.
- **Reusable workflows** — start from focused delivery, debugging, review, and testing prompts; provider commands and local skills are available from chat.
- **Themes** — Graphite, Dusk, Paper, System, and an optional Glass appearance.
- **Sites surface** — presentation-only Sites UI with search, refresh, and create hooks. Persistence and execution are intentionally not implemented here.

## Requirements

The current developer build is exercised on macOS Apple Silicon.

- Go 1.26+
- CGO enabled
- Node.js 20+
- npm
- Wails CLI 2.10.2
- Git
- At least one supported agent CLI installed and authenticated

Install Wails at the version used by the project:

```sh
go install github.com/wailsapp/wails/v2/cmd/wails@v2.10.2
```

Supported agent executables:

| Provider | Executable | Native chat | Direct TUI | Resume |
|---|---|---:|---:|---:|
| Claude Code | `claude` | Yes | Yes | Yes |
| Codex CLI | `codex` | Yes | Yes | Yes |
| GitHub Copilot CLI | `copilot` | Initial prompt | No | No |
| OpenCode | `opencode` | Initial prompt | No | No |
| Local shell | `$SHELL` | No | Terminal | No |

Optional integrations:

- [GitHub CLI](https://cli.github.com) (`gh auth login`) for pull-request listing and creation.
- Jira CLI (`jira`) for live ticket metadata.

OpenADE does not proxy or store provider credentials. Each integration uses the CLI already authenticated on the machine.

## Build and run

```sh
git clone https://github.com/gitkamaal/OpenADE.git
cd OpenADE/apps/desktop
npm ci
CGO_ENABLED=1 "$(go env GOPATH)/bin/wails" build -clean
open build/bin/OpenADE.app
```

The packaged application starts or reconnects to the daemon at `127.0.0.1:7433`.

For native development:

```sh
cd apps/desktop
npm ci
CGO_ENABLED=1 "$(go env GOPATH)/bin/wails" dev
```

For browser-only frontend work, run the daemon and Vite separately:

```sh
# terminal 1
cd apps/desktop
go run . --daemon --addr 127.0.0.1:7433

# terminal 2
cd apps/desktop
npm run dev
```

## First session

1. Open **Settings** and choose the default agent and Native chat or Direct TUI.
2. Select a workspace root to populate the Projects sidebar with repositories and resumable provider conversations.
3. Start a task from Home or choose a reusable workflow.
4. Select a Git repository and base branch.
5. Optionally add a Jira key such as `ADE-123` and its ticket URL.
6. Submit the task. OpenADE creates the worktree and task branch before launching the agent.

While an agent is working, follow-up messages can be queued, steered to the front, edited, or removed. The right sidebar opens Changes, Terminal, Pull Request, and Ticket without reducing the main conversation to a narrow column.

## Architecture

```text
┌──────────────────────────────────────────────────────────┐
│ Wails 2 desktop                                          │
│ React 19 + TypeScript + xterm.js                         │
│ Native chat, Direct TUI, projects, review, PRs, settings │
└────────────────────────────┬─────────────────────────────┘
                             │ localhost HTTP + WebSocket
┌────────────────────────────▼─────────────────────────────┐
│ One Go daemon                                            │
│ SQLite WAL index · message queues · transcripts          │
│ PTY/process-group ownership · terminal/session streaming │
│ Git worktrees · GitHub CLI · Jira CLI                    │
└───────────────┬──────────────────────────┬───────────────┘
                │                          │
       ┌────────▼────────┐        ┌────────▼────────┐
       │ Agent CLIs      │        │ Git repositories│
       │ Claude / Codex  │        │ + worktrees     │
       │ Copilot/OpenCode│        └─────────────────┘
       └─────────────────┘
```

The daemon is deliberately independent from the window lifecycle:

- Wails starts it only when no healthy daemon is already listening.
- Closing the window leaves it and its managed PTYs running.
- Reopening the app reattaches to live sessions and terminal scrollback.
- Daemon shutdown closes PTYs, subscribers, transcript writers, process groups, and SQLite in a deterministic order.

## Local data

By default, state is stored in the operating system's user configuration directory. On macOS this is:

```text
~/Library/Application Support/OpenADE/
```

The directory contains:

```text
openade.sqlite3       session, queue, and terminal index
openade.sqlite3-wal   SQLite write-ahead log while active
worktrees/            isolated task checkouts
transcripts/          agent PTY transcripts
terminal-transcripts/ independent shell transcripts
daemon.log            detached daemon output
```

Configuration knobs:

| Variable | Purpose | Default |
|---|---|---|
| `OPENADE_DATA_DIR` | Override daemon state and worktree storage | OS user config directory |
| `OPENADE_DAEMON_ADDR` | Override daemon listen address | `127.0.0.1:7433` |
| `VITE_OPENADE_DAEMON_URL` | Point the browser frontend at another local daemon | `http://127.0.0.1:7433` |

## Testing

Backend tests use Go's standard `testing` package. Frontend tests use Vitest 3 with jsdom and Testing Library. End-to-end tests use Playwright 1.62 against a real Go daemon, Vite frontend, fixture Git repository, PTYs, and Chromium.

```sh
cd apps/desktop

# Go unit and daemon integration tests, including race detection
go test -race ./...
go vet ./...

# React unit and component integration tests
npm test

# Production frontend type-check and build
npm run build

# Browser lifecycle tests
npx playwright install chromium # first run only
npm run e2e

# Packaged desktop build
CGO_ENABLED=1 "$(go env GOPATH)/bin/wails" build -clean
```

The lifecycle suites specifically exercise:

- PTY closure and transcript flushing after a process exits.
- Subscriber removal and channel closure.
- Managed process-group termination during daemon shutdown.
- Non-overlapping polling and stale async response suppression.
- WebSocket reconnect-timer cleanup.
- xterm input subscriptions, ResizeObservers, sockets, and terminal disposal.
- Repeated Direct TUI navigation and multiple project-terminal open/close cycles in Chromium.

Current verified checkpoint on this branch:

- Go race tests and `go vet` pass.
- 20 Vitest files / 130 tests pass on Vitest 3.2.7.
- Repeated Playwright lifecycle runs pass on Playwright 1.62.1.
- Wails 2.10.2 produces a macOS arm64 application bundle with CGO enabled.

## Repository layout

```text
apps/desktop/
  main.go                 Wails application and standalone daemon entrypoint
  app.go                  desktop-to-daemon lifecycle bridge
  internal/daemon/        sessions, terminals, SQLite, Git, GitHub, and Jira
  src/ade/                current React desktop experience
  e2e/                    Playwright fixture world and lifecycle coverage
  build/bin/OpenADE.app   local production output

crates/                   earlier Rust daemon/server prototype
docs/                     product and historical design documentation
```

## Current limitations

- Direct TUI and durable provider resume are implemented only for Claude Code and Codex CLI.
- Sites is a UI integration surface only.
- Jira support expects a locally installed and authenticated `jira` executable.
- GitHub operations expect a locally installed and authenticated `gh` executable and an `origin` repository you can push to.
- The current frontend bundle emits a non-blocking large-chunk warning; code splitting is a future performance pass.
- Several documents and screenshots under `docs/` describe the earlier Rust/Tauri prototype and may not match this branch's current UI.

## Principles

- **Local first.** Agent processes, transcripts, state, and worktrees stay on the machine.
- **Bring your own agent.** Authentication remains with each provider's CLI.
- **Isolation by default.** Parallel tasks do not share a mutable checkout.
- **The daemon owns execution.** UI navigation and window lifecycle never define process lifetime.
- **Review before delivery.** Diffs, linked tickets, branches, and draft pull requests remain connected to the session that produced them.

## License

[Apache-2.0](LICENSE)

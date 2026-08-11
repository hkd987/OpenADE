# PRD: Open Agentic Development Environment ("OpenADE")

**Version:** 0.1 (Draft)
**Date:** 2026-08-11
**Author:** Lundin Matthews
**Status:** For engineering review
**Working codename:** OpenADE (placeholder — rename before repo creation)

---

## 1. Summary

Build an open-source, vendor-neutral agentic development environment modeled on Spotify's Xirp (launched Aug 10, 2026, closed beta, macOS-only, requires a Spotify account and their commercial Portal product for the context layer).

The product is a desktop app + context service that:

1. **Orchestrates parallel AI coding agent sessions** (Claude Code, Codex CLI, Gemini CLI) in isolated Git worktrees from a single control surface.
2. **Grounds every session in organizational context** pulled from a **Backstage** software catalog (services, ownership, dependencies, ADRs, TechDocs) via **MCP** — with the catalog backend abstracted so other IDP platforms (Port, Cortex, OpsLevel, or a plain Git-backed catalog) can be swapped in later.
3. **Captures session knowledge** and feeds it back — session transcripts are summarized into living documentation attached to catalog entities, so every session makes the next one smarter.

Xirp's strategic wedge is that the harness is free but the context layer (Portal) is commercial and closed. Our wedge is the inverse: **the context layer is open and pluggable**, starting with open-source Backstage, which thousands of orgs already run.

### Reference product (Xirp)

- Landing page: https://xirp.spotify.com/
- Docs: https://backstage.spotify.com/docs/xirp (docs index: https://backstage.spotify.com/docs/llms.txt)
- Launch blog: https://portal.spotify.com/blog/introducing-xirp
- Launch announcement: https://x.com/SpotifyEng/status/2086795659651191106

### Reference screenshots (for design/UX study)

| What it shows | Link |
|---|---|
| Xirp home screen (docs) | https://mintcdn.com/spotify-89f50c35/Au6-kNeho5Rk8Vno/xirp/assets/xirp-home.webp |
| Desktop app — coding session w/ project context | https://xirp.spotify.com/_next/static/media/home-app-desktop.0on4omda4vxrz.webp |
| Workspace plugin inside Portal | https://xirp.spotify.com/_next/static/media/home-app-portal.3s6b82prncf7a.webp |
| Agent session grounded in system context | https://xirp.spotify.com/_next/static/media/feature-agent.1wd_g1-gw11st.webp |
| Shared team sessions (Workspace) | https://xirp.spotify.com/_next/static/media/feature-workspace.2placxw-mk1jo.webp |
| Auto-generated living documentation | https://xirp.spotify.com/_next/static/media/feature-documentation.3jiawz-5nywbd.webp |
| Model/harness selector (Claude / Gemini / Codex) | https://xirp.spotify.com/_next/static/media/feature-flexibility.1w469rn3uqiy_.webp |
| Hero: catalog indexing illustration | https://xirp.spotify.com/_next/static/media/hero-illustration.12uw-w-cy6c8f.svg |

> Note: these are Spotify's marketing/docs assets — use for competitive study only. Nothing in our UI should copy their visual design; their trademarks and assets stay out of our repo.

---

## 2. Problem Statement

Engineers now run many concurrent coding agents, and two problems compound at scale:

1. **Orchestration.** Running one agent is a workflow; running ten is an operational mess — sessions collide on the same checkout, terminal state is lost, and every harness (Claude Code, Codex, Gemini CLI) has its own config, rules format, and session model. Xirp validated this pain: 1,300+ Spotify engineers and 36,000+ internal sessions before public beta.
2. **Context.** Agents make fast, confident decisions that are technically correct and operationally wrong because they can't see the system around the file: who owns the upstream service, what depends on this, why it was built this way. That knowledge lives in catalogs, ADRs, and people's heads — not in the repo the agent can see.

The only integrated solution (Xirp) locks the context layer to Spotify's commercial Portal SaaS, is macOS-only, closed beta, and has unanswered questions about data usage and open-sourcing. Orgs that already run open-source Backstage — or any other catalog — have no way to make that investment pay off in agent sessions.

## 3. Goals

1. **G1 — Parallel session orchestration:** An engineer can run 10+ concurrent agent sessions across 3 harnesses on one repo with zero worktree/branch collisions, from a single window.
2. **G2 — Context-grounded sessions:** A session launched against a catalog entity automatically has ownership, dependency, API, and docs context available to the agent via MCP — measurably reducing "agent asked a question the catalog could answer" events.
3. **G3 — Compounding knowledge:** ≥50% of completed "workspace" sessions produce a reviewed knowledge artifact (session summary / doc update) attached to a catalog entity within 2 minutes of session end.
4. **G4 — Vendor neutrality at two layers:** Switch harness mid-task without losing working state (worktree, rules, context bundle carry over), and switch catalog backend (Backstage → other) behind a stable provider interface.
5. **G5 — Community traction (open source):** 500 GitHub stars and ≥3 non-founder contributors within 90 days of public v0.1; ≥5 orgs running it against a real Backstage instance within 6 months.

## 4. Non-Goals

- **Building or hosting a coding agent/model.** We orchestrate existing harnesses; users authenticate each agent through its native CLI (same posture as Xirp). No API-key proxying in v1.
- **Replacing source control or CI.** Sessions operate on local clones/worktrees; PR creation goes through the user's normal Git flow. No autonomous background/CI agents in v1 (P2).
- **Building an IDP/catalog.** We consume Backstage; we do not fork it or re-implement catalog ingestion. Orgs bring their own catalog.
- **A hosted SaaS.** v1 is self-hosted/local. A managed offering is a future business decision, not a v1 requirement (this also keeps the open-core question out of scope for now).
- **Windows/Linux desktop parity in v0.1.** Architecture must be cross-platform (see Tauri decision), but v0.1 ships macOS-first with Linux as fast-follow. (Xirp is macOS-only; matching that at launch is acceptable, beating it is a differentiator.)
- **Fine-grained agent governance/policy enforcement.** Credential enforcement, binary authorization, and audit-grade controls are a separate product layer and explicitly out of scope here; we should design session telemetry so that layer can attach later.

## 5. Target Users & User Stories

**Personas:** IC engineer running agents daily ("Operator"), tech lead curating team context ("Curator"), platform/DevX engineer administering the catalog ("Platform").

**P0 stories**

- As an **Operator**, I want to start a session on any local repo with my choice of harness so that I don't juggle three terminal setups.
- As an **Operator**, I want each session in its own Git worktree so that parallel agents never fight over a checkout.
- As an **Operator**, I want a grid view of all live sessions with status (working / waiting on input / done / error) so I can supervise many agents at once.
- As an **Operator**, I want to launch a session from a catalog entity so the agent knows the service's owner, dependencies, APIs, and docs without me pasting context.
- As an **Operator**, I want my rules and skills applied consistently across harnesses so behavior doesn't change when I switch models.
- As a **Curator**, I want completed sessions summarized and attached to the entity so the next engineer (or agent) starts from what we learned.
- As a **Platform**, I want to connect the app to our existing Backstage instance with a URL + token so adoption doesn't require new infrastructure.

**P1 stories**

- As an **Operator**, I want to hand a running task from one harness to another (e.g., Claude → Gemini) with working state intact.
- As a **Curator**, I want to review/edit auto-generated docs before they publish to TechDocs.
- As a **Platform**, I want session metadata (entity, harness, duration, files touched) queryable so I can measure agent activity per system.

## 6. Product Requirements

### P0 — Must have (v0.1–v0.3)

**R1. Session manager**
- Spawn harness CLIs (Claude Code, Codex CLI, Gemini CLI) as persistent PTY sessions that survive window close/reopen (daemonized session host, tmux-style).
- Session states: idle / running / needs-input / completed / failed; surfaced in a grid dashboard.
- Acceptance: kill and reopen the app; all sessions reattach with full scrollback.

**R2. Worktree isolation**
- One-click "new task" creates `git worktree add` on a task branch; session is bound to that worktree; cleanup on task close (with dirty-state guard).
- Acceptance: 10 simultaneous sessions on one repo produce zero cross-session file conflicts.

**R3. Unified control surface**
- Per-session: terminal, diff view (worktree vs. base branch), file browser, rules/skills panel.
- Global: session grid, project list, harness picker.

**R4. Harness adapter layer**
- Adapter interface per harness: launch command, resume semantics, rules file mapping, MCP registration mechanism, transcript location.
- Rules normalization: one canonical rules/skills source in the project, materialized to `CLAUDE.md` / `AGENTS.md` / `GEMINI.md` (symlink or generated) so all harnesses see equivalent instructions.
- Acceptance: same task brief runs on all three harnesses with no manual per-harness config beyond native CLI auth.

**R5. Backstage context provider (MCP)**
- An MCP server ("catalog-mcp") exposing read tools against the Backstage REST API: `get_entity`, `get_owner`, `get_dependencies` (relations graph), `search_catalog`, `get_techdocs_page`, `get_apis_for_entity`.
- Session launch from an entity pre-injects a compact "context bundle" (entity summary, owner, top dependencies, links to ADRs/docs) into the session's system context, with deeper retrieval available via MCP tools on demand.
- Auth: static token or OAuth against the org's Backstage; provider interface (`CatalogProvider` trait) so non-Backstage backends can implement the same tool surface later.
- Note: recent Backstage releases include first-party MCP/actions backend work — evaluate building on that vs. our own thin server (Open Question Q2).

**R6. Session capture → knowledge artifact**
- Structured session log (JSONL: prompts, tool calls, diffs, outcomes).
- On session end, a summarization pass (user's own configured model) produces: what changed, why, decisions made, gotchas discovered.
- Artifact is attached to the catalog entity (v0: committed as markdown into the entity's TechDocs source / `docs/` dir via PR; human reviews before merge).
- Acceptance: end-to-end session → reviewed doc PR in under 5 minutes of user effort.

### P1 — Should have

- Cross-harness handoff (context bundle + worktree carry over; new harness resumes from summary).
- Shared workspace: a **Backstage frontend plugin** listing sessions/artifacts per entity (this is our answer to Xirp's Portal "Workspace" plugin — ours lives in open-source Backstage).
- Session artifact search fed back into context bundles ("prior sessions on this entity").
- Linux build.

### P2 — Future considerations (design for, don't build)

- Remote/headless session execution (sessions on a devbox, UI attaches).
- Additional catalog providers (Port, Cortex, Git-native YAML catalog).
- Cost/model routing across harnesses.
- Governance hooks: session telemetry export for policy/audit tooling.
- Team/multi-user server mode.

## 7. Technical Design

### 7.1 Architecture

```
┌─────────────────────────────────────────────────────┐
│ Desktop app (Tauri: Rust core + TS/React UI)        │
│  • Session grid, terminal (xterm.js), diff view     │
└──────────────┬──────────────────────────────────────┘
               │ IPC
┌──────────────▼──────────────────────────────────────┐
│ Session daemon (Rust)                               │
│  • PTY host (portable-pty), session persistence     │
│  • Worktree manager (git2 / gix)                    │
│  • Harness adapters (claude / codex / gemini)       │
│  • Transcript recorder (JSONL, SQLite index)        │
└───────┬─────────────────────────────┬───────────────┘
        │ spawns + MCP config          │ HTTP
┌───────▼───────────┐        ┌────────▼───────────────┐
│ Harness CLIs      │  MCP   │ catalog-mcp (Rust)     │
│ Claude Code       │◄──────►│  CatalogProvider trait │
│ Codex CLI         │        │  └ BackstageProvider   │
│ Gemini CLI        │        │    (REST /api/catalog, │
└───────────────────┘        │     TechDocs, search)  │
                             └────────┬───────────────┘
                                      │
                             ┌────────▼───────────────┐
                             │ Backstage instance     │
                             │ (org-owned, existing)  │
                             └────────────────────────┘
```

### 7.2 Key technical decisions & rationale

| Decision | Choice | Rationale |
|---|---|---|
| Desktop shell | **Tauri 2** | Rust core matches team expertise; small binaries; cross-platform path to Linux/Windows without Electron weight. |
| Terminal | **xterm.js + portable-pty** | Proven combo; PTY host lives in the daemon so sessions outlive the UI. |
| Session persistence | Daemon process + SQLite | Reattachable sessions are table stakes (Xirp's "persistent terminal sessions"). |
| Git ops | `gitoxide`/`git2` + shelling out for `worktree` | Worktree edge cases are safer via git CLI; libs for status/diff. |
| Context transport | **MCP (stdio + streamable HTTP)** | All three harnesses support MCP servers; it's the vendor-neutral seam Xirp itself uses ("context through MCP"). |
| Catalog | **Backstage first**, behind `CatalogProvider` | Largest installed base; open source; his-and-hers with our future providers. |
| Knowledge write-back | Markdown → TechDocs via PR | Human-in-the-loop, Git-auditable, zero new storage. |
| License | Apache-2.0 (align with Backstage/CNCF norms) | Maximizes adoption; revisit open-core only if a hosted product emerges (deliberately out of scope). |

### 7.3 Backstage integration specifics

- **Read path:** Backstage Catalog REST API (`/api/catalog/entities/by-name/...`, `/entities/by-query`, relations for `dependsOn` / `ownedBy` / `providesApi`), TechDocs static site content, and Search API. All read-only in v0.
- **Context bundle format:** a versioned JSON+markdown bundle (entity card, owner + contact, N nearest dependencies, API surfaces, links to ADR/TechDocs pages, prior session summaries). Budgeted to ~2–4K tokens injected; everything else on-demand via MCP tools.
- **Write path (P0.5):** knowledge artifacts land as PRs to the entity's docs source. Direct TechDocs publishing and a Backstage frontend plugin ("Sessions" tab per entity) are P1.
- **Auth:** Backstage static tokens (service-to-service) for v0; guest/OAuth flows documented for orgs that need them.

### 7.4 Harness adapter notes

| Harness | Session resume | Rules file | MCP registration |
|---|---|---|---|
| Claude Code | `claude --resume` / session ids | `CLAUDE.md` (+ skills dirs) | `claude mcp add` / project `.mcp.json` |
| Codex CLI | resume support via session state | `AGENTS.md` | config file MCP entries |
| Gemini CLI | checkpoint/resume | `GEMINI.md` | `settings.json` mcpServers |

Adapters own the mapping; verify exact flags per current CLI versions during Phase 1 spike (these tools change monthly — treat the table as direction, not gospel).

### 7.5 Security & privacy

- No telemetry by default; opt-in anonymous usage stats only.
- Agent credentials never touch our code — native CLI auth only.
- Catalog tokens stored in OS keychain.
- Transcripts stay local (SQLite + files) unless the user explicitly publishes an artifact.
- This is also our positioning answer to the data-usage concerns raised publicly about Xirp's account requirement.

## 8. Milestones & Phasing

| Phase | Scope | Target |
|---|---|---|
| **0 — Spike (2 wks)** | Harness adapter feasibility: PTY persistence, resume semantics, MCP registration for all 3 CLIs; Backstage API read spike against a demo instance | Go/no-go on adapter layer |
| **1 — Local ADE (6 wks)** | R1–R4: sessions, worktrees, grid, adapters. Usable with zero Backstage. | Private alpha, dogfood |
| **2 — Context (4 wks)** | R5: catalog-mcp, entity-launched sessions, context bundles | v0.1 public OSS release |
| **3 — Knowledge loop (4 wks)** | R6: capture, summarize, doc PRs | v0.2 |
| **4 — Workspace (6 wks)** | Backstage frontend plugin, prior-session retrieval, handoff, Linux | v0.3 |

Ship Phase 1 publicly even before context lands — a good OSS multi-harness session manager has standalone demand (Xirp explicitly works Portal-free for the same reason).

## 9. Success Metrics

**Leading (weeks):** sessions/user/week; concurrent-session p90; % sessions launched from an entity; context-bundle tool-call rate; artifact publish rate per workspace session.
**Lagging (months):** GitHub stars/contributors (G5 targets); # orgs with Backstage connected; retention of weekly-active operators at 8 weeks; qualitative "agent had the context" wins vs. baseline.
**Measurement:** local opt-in metrics + GitHub analytics; define event schema in Phase 1.

## 10. Open Questions

- **Q1 (Eng, blocking Phase 0):** Do current Codex CLI and Gemini CLI resume semantics actually support reattachable persistent sessions, or do we fake persistence at the PTY layer only?
- **Q2 (Eng, Phase 2):** Build on Backstage's first-party MCP/actions backend work vs. our own standalone catalog-mcp server? (Ours decouples us from Backstage release cadence; theirs reduces maintenance.)
- **Q3 (Eng/Design):** Context bundle token budget — fixed injection vs. fully tool-call-driven retrieval? Needs empirical testing per harness.
- **Q4 (Legal, non-blocking):** Naming and trademark review (no "Xirp"/"Portal"/"Backstage" in the product name; "for Backstage" descriptor usage per CNCF/Spotify trademark guidelines).
- **Q5 (Product):** Is the summarization model for knowledge artifacts the session's own harness (zero new config) or a user-configured endpoint?
- **Q6 (Product, non-blocking):** GitHub-only for catalog/docs write-back in v0, or GitLab too?

## 11. Competitive Notes

- **Xirp:** the reference. Closed beta, macOS-only, Spotify account required, context layer requires commercial Portal. Our differentiators: open source, open context layer (bring-your-own Backstage), no account, local-first privacy, cross-platform path.
- **Aaron Francis's "Solo" and similar session-manager tools:** validate individual-scale demand for the orchestration half; none have the catalog/context half — that's the moat and the reason Phase 2 is the point of this project, not Phase 1.
- **Harness-native features** (Claude Code subagents/teams, etc.): labs will keep improving single-harness orchestration; our defensibility is the cross-harness + org-context combination, not any single feature.

---

*Next artifacts on request: Phase 0 spike ticket breakdown, catalog-mcp tool schema draft, ADR-001 (Tauri vs. Electron vs. TUI), repo scaffold.*

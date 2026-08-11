# Design notes — Xirp reference study

**Date:** 2026-08-11. Studied the seven reference screenshots linked in
[PRD §1](PRD.md) (Xirp desktop app, agent session, workspace, documentation,
model picker, Portal project view). Per the PRD's trademark note, the images
were reviewed locally only — **no Spotify assets are committed to this
repository**, and OpenADE deliberately keeps its own visual identity (dark,
minimal) rather than imitating Xirp's light/orange design.

## Structural takeaways adopted

| Xirp pattern observed | OpenADE implementation |
|---|---|
| Sidebar groups sessions **by project**, with per-project headers | Session grid groups by repository with uppercase project headers (`projectName(repo_root)`) |
| Session cards: branch ref + status ("Working" / "Idle"), tinted active card | Cards show branch, entity, and state badge; our vocabulary is richer (`running` / `needs-input` / `completed` / `failed`) because needs-input is a first-class grid state (R1) |
| Workspace session list shows author + model + relative age | Cards show harness + compact relative age ("5m ago"); author becomes relevant in shared-daemon mode (P2) |
| Model picker attributes each harness to its vendor | Harness picker options read "Claude Code · Anthropic", etc. |
| Session header: harness selector, elapsed time, Stop | Detail header: handoff harness selector, Kill; elapsed time on the card |
| Living documentation is a **navigable wiki with an index** (grouped links + one-line summaries) | `docs/openade/sessions/index.md` is regenerated on every artifact publication — newest-first links with summaries, harness, and date — so merged knowledge stays navigable, not a pile of files |
| Portal project page: Sessions tab per project/entity | `GET /sessions?entity=<ref>` is the data source; the Backstage frontend plugin renders it (P1 roadmap) |

## Deliberate differences

- **Dark, quiet UI** instead of Xirp's light/orange: distinct identity and a
  trademark-safe distance; terminals read better on dark ground.
- **Status vocabulary**: Xirp shows Working/Idle; OpenADE separates
  `needs-input` from `running` because unblocking a waiting agent is the #1
  supervision action, and distinguishes `completed` from `failed`.
- **Session titles are tasks, not auto-names**: Xirp shows "Session /
  session/scheming-hawk-jhgk"; OpenADE keeps the operator's task title as
  the primary label with the generated branch as secondary metadata.
- **Docs are Git-reviewed**, not auto-published: our index and artifacts land
  on `openade/knowledge-*` review branches (PRD R6 human-in-the-loop).

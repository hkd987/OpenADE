# Design QA — Tembo reference / OpenADE Wails build

**Status:** Passed  
**Date:** 2026-08-20  
**Reference state:** Tembo home, authorized workspace capture  
**Implementation state:** OpenADE home, production Wails build with indexed live test sessions

## Comparison

- Reference: `docs/tembo-reference/home.png`
- Implementation capture: `../outputs/openade-home-final.jpeg`
- Same-view comparison: `../outputs/tembo-openade-home-comparison.png`
- Comparison viewport: both inputs normalized to 1194 × 768, top aligned

## Findings

- **P0:** none. The core session composer, navigation, recents, active-session area, and daemon status are visible and usable.
- **P1:** none. Desktop hierarchy, sidebar proportions, dark surface system, restrained borders, compact typography, and primary composer focus match the reference direction.
- **P2:** none. Empty state, repository picker, agent control, recents density, focus states, and responsive breakpoints are coherent and free of clipping or overlap.
- **P3 / intentional:** OpenADE replaces Tembo's onboarding cards with local-agent trust indicators and a live session region. It keeps a single dark theme across session/review views so terminal, diff, ticket, and PR states feel continuous in the native shell.

## Functional visual states checked

- Home and repository selection
- Session index and status filtering
- Agent template library
- GitHub review queue with a real draft PR
- Ticket-linked completed session and PR panel
- Structured Codex response with streamed intent, command output, and final answer
- Native PTY terminal surface
- Reduced-width responsive shell from the reference capture

No blocking visual defects remain for this implementation pass.

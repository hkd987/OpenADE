# Agent session UX design QA

## Evidence

- Source visual truth: `/var/folders/2h/k_vpn3_n7kl2knmtx_w78s940000gn/T/codex-clipboard-2d6a6693-5799-4682-be5e-417374d6d4b6.png` (ChatGPT-style activity and chat), `/var/folders/2h/k_vpn3_n7kl2knmtx_w78s940000gn/T/codex-clipboard-feeb6b87-5d7f-43f3-a7ac-557904bbc7d7.png` (composer control baseline), and `/var/folders/2h/k_vpn3_n7kl2knmtx_w78s940000gn/T/codex-clipboard-5b3bdadb-310a-4033-8196-cbe908f637ff.png` (prior OpenADE split workspace).
- Rendered implementation: `../outputs/openade-native-chat-thinking-collapsed.png`, `../outputs/openade-native-chat-complete-collapsed.png`, `../outputs/openade-direct-codex-tui.png`, `../outputs/openade-skills-commands-menu.png`, and `../outputs/openade-settings-tui-projects.png`.
- Full-view comparison: `../outputs/openade-chat-reference-comparison.png`.
- Focused composer comparison: `../outputs/openade-composer-reference-comparison.png`.
- Viewport: source chat 1017 × 604 px; implementation 1194 × 768 px Wails window. The implementation main-content crop was normalized to 1017 × 604 without scaling for the combined comparison.
- Density: 1× screenshots; no retina-density conversion was needed.
- State: live Codex native-chat turn with collapsed thinking, completed native-chat turn, completed/resumable direct Codex TUI, skills menu open, and Glass settings.

## Findings

- P0: none.
- P1: none.
- P2: none.
- P3: the native-chat implementation keeps the provider and conversation status in the composer but does not duplicate ChatGPT’s model selector and microphone. OpenADE exposes the harness default in Settings and preserves the provider CLI’s own model controls in Direct TUI, so this is an intentional product split.

## Required fidelity surfaces

- Fonts and typography: Inter remains the interface face and the provider TUI uses a native monospace stack. Agent labels, activity summaries, Markdown, footer status, and command descriptions maintain distinct optical weights and do not wrap into adjacent controls.
- Spacing and layout rhythm: the live activity summary remains one compact row; the activity icon and 18 px text row share a centered baseline. Composer controls share a 32 px baseline with 12 px left padding and 9 px right padding. The direct TUI fills the primary session surface rather than appearing inside the independent-shell panel.
- Colors and tokens: the Glass theme retains a cool navy field, restrained edge highlights, and readable semantic status colors. Terminal content remains opaque near-black for predictable ANSI contrast.
- Image quality and assets: no raster placeholders or approximation assets were introduced. All controls use the existing Phosphor icon system, and xterm renders the real provider terminal output.
- Copy and content: “Native chat,” “Direct TUI,” “Completed activity,” “Workspace folder,” and “Skills & commands” name user-recognizable choices. The composer status is short and vertically aligned.

## Comparison history

1. Initial implementation: blocked by three P1 issues observed in the live app: streaming activity auto-expanded, Direct TUI opened the independent-shell panel instead of the provider PTY, and the composer footer had no commands/skills entry point.
2. First correction: activity stayed collapsed while running and the footer baseline was corrected. Direct TUI still inherited the terminal-panel default and hid its primary PTY.
3. Second correction: Direct TUI became the primary session surface, survived desktop restart, accepted real Codex keyboard input, and exposed Resume after stopping. The provider-aware commands menu and workspace-root settings were then verified.
4. Project continuity correction: the workspace scan now finds local Codex and Claude histories, groups them with the matching repository, combines them into Recents, and resumes the original provider conversation in a new isolated worktree.
5. Final comparison: no actionable P0/P1/P2 mismatch remains. The implementation intentionally shows the requested collapsed running state, whereas the source chat screenshot shows a completed expanded state.

## Interactions tested

- Created a real Codex Direct TUI session in an isolated worktree.
- Completed Codex’s hook-trust prompt through xterm input.
- Closed and relaunched the desktop shell while the daemon-owned PTY remained attached.
- Stopped the TUI, verified completed status, transcript replay, and the Resume affordance.
- Created a native Codex chat and verified the live “Thinking” row stayed collapsed and clickable.
- Verified the native response, Markdown surface, copy action, plus button, footer status, and send button alignment.
- Opened the provider-aware skills menu and verified local Codex/shared/project skills, descriptions, filtering, and invocation syntax.
- Verified Glass, Native chat/Direct TUI, compact activity, harness default, and workspace-folder controls in Settings.
- Verified provider-history parsing, repository matching, sidebar rendering, and one-click Direct TUI resume with automated regression coverage.
- Checked the macOS accessibility tree after each primary interaction; no clipped persistent controls or inaccessible primary actions were found.

## Final result

passed

---

# Durable message queue extension QA

## Evidence

- Source visual truth: `/var/folders/2h/k_vpn3_n7kl2knmtx_w78s940000gn/T/codex-clipboard-4c40680a-445d-4965-98e2-959ea374c7ae.png` (902 × 293 px queue and composer reference).
- Rendered implementation: `../outputs/openade-message-queue-native.jpg` (1,236 × 768 px native Wails window) and `../outputs/openade-message-queue-drained.jpg` (completed daemon-owned queue after the desktop shell was closed and reopened).
- Focused comparison: `../outputs/openade-message-queue-comparison.jpg`; the 710 × 225 px implementation queue/composer crop was normalized to the 902 × 293 px source at 1× density.
- Full-view evidence: `../outputs/openade-message-queue-native.jpg`; a separate full-view comparison was unnecessary because the requested visual target is only the focused composer/queue component.
- State: Codex actively streaming with two queued messages, second item steered to the front, followed by a completed queue with no pending rows.

## Findings

- P0: none.
- P1: none.
- P2: none after correction.
- P3: the reference composer includes model, microphone, and access controls from its host product. OpenADE retains its existing provider status and skills controls because model/permission selection belongs to the configured local harness and Direct TUI.

## Required fidelity surfaces

- Fonts and typography: compact Inter rows, muted progress copy, single-line truncation, and low-emphasis action labels reproduce the reference hierarchy without colliding at the 760 px composer width.
- Spacing and layout rhythm: the queue tray is inset 12 px from the composer, overlaps its top edge by 1 px, uses 30 px rows, and keeps the progress pill centered 43 px above the queue. The composer remains the dominant surface beneath it.
- Colors and tokens: queue surfaces, hover states, focus treatment, borders, muted labels, and Glass blur use the established OpenADE tokens. The cool Glass palette intentionally replaces the source application's warm Dusk palette.
- Image quality and assets: the source contains only interface icons. All queue, steer, delete, and overflow controls use the existing Phosphor icon family; no placeholder image, custom SVG, or CSS-drawn asset was introduced.
- Copy and content: “Step 1 / 3,” “Steer,” the queued prompts, queue count, and composer placeholder communicate order and state without implementation terminology.

## Comparison history

1. The first implementation placed the queue inside the full-width composer border. The source uses a narrower card attached above the composer, making the queue feel subordinate to the active input (P2).
2. The correction introduced a dedicated composer dock, inset the queue by 12 px, overlapped the two surfaces by 1 px, and preserved the centered progress pill. The final focused comparison has no actionable P0/P1/P2 mismatch.

## Interactions tested

- Added two messages while a real Codex turn was running and verified immediate ordered queue rendering.
- Steered the second item to the front and verified visual and daemon ordering.
- Verified remove and edit-to-composer behavior through component coverage.
- Verified a dispatching row disables destructive controls and exposes “Sending” state.
- Closed the desktop shell with two queued messages while a 12-second Codex turn was active.
- Reopened OpenADE after the daemon completed both queued turns and verified both user prompts and assistant responses in transcript order.
- Verified the daemon persists queue rows in SQLite, resets interrupted dispatch state on restart, and drains one provider turn at a time.
- Checked the macOS accessibility tree for the queue region, progress indicator, Steer controls, removal labels, editing labels, and composer state.

## Final result

passed

---

# Seamless macOS shell and session inspector QA

## Evidence

- Source visual truth: `/var/folders/2h/k_vpn3_n7kl2knmtx_w78s940000gn/T/codex-clipboard-5a5977c9-1081-46e8-9448-ab5ec000d2fa.png` (full-height ChatGPT desktop chrome and right utility rail), `/var/folders/2h/k_vpn3_n7kl2knmtx_w78s940000gn/T/codex-clipboard-c176c88f-dd64-42bd-935d-23db5b22a1ef.png` (prior top-right work tabs), and `/var/folders/2h/k_vpn3_n7kl2knmtx_w78s940000gn/T/codex-clipboard-11bd8f42-5890-4a56-99c5-1d5600625587.png` (collapsed-sidebar clipping defect).
- Rendered implementation: `../outputs/openade-session-inspector-closed.jpg`, `../outputs/openade-session-inspector-open.jpg`, and `../outputs/openade-sidebar-collapsed-clean.jpg` at 1,236 × 768 px in the native Wails app.
- Combined reference/implementation input: `../outputs/openade-shell-comparison.jpg`; desired seamless chrome and the prior collapse defect are paired with the corrected inspector-open and sidebar-collapsed states.
- State: completed Codex chat, PR inspector closed/open, sidebar closed/restored, Glass theme.

## Findings

- P0: none.
- P1: none after correction. Native testing caught the unmounted workspace being auto-placed into the obsolete zero-width sidebar column; the collapsed grid now removes that column entirely.
- P2: none.
- P3: OpenADE keeps a labeled 58 px developer-tool rail rather than ChatGPT's narrower icon-only output rail. The labels reduce ambiguity among Changes, Terminal, and PR and are intentional for a multi-agent development workspace.

## Required fidelity surfaces

- Native chrome: the Wails macOS window uses an inset hidden title bar with full-size content and no toolbar separator. Traffic lights sit inside the sidebar or workspace field instead of in a separate title strip.
- Sidebar behavior: closing the sidebar unmounts its background, padding, border, scroll area, and descendants. The workspace receives the full grid width; the restore control remains clear of the traffic lights.
- Work-surface hierarchy: session status and actions remain in the drag-capable header, while Changes, Terminal, and PR live in a persistent right inspector rail. Selecting a tool opens its panel; selecting the active tool or its close action dismisses the panel.
- Layout rhythm: the session header reserves 20 px of titlebar inset, the inspector rail is 58 px, and the conversation/panel columns use zero-safe minmax sizing so neither side clips or forces horizontal overflow.
- Accessibility: the rail is named “Session tools,” each tool exposes pressed state, each open panel has a distinct accessible name, and the sidebar restore action remains keyboard-addressable.

## Interactions tested

- Opened a real indexed Codex session in the packaged native app.
- Opened and closed the PR inspector from the right rail; verified selected state and panel naming through the macOS accessibility tree.
- Collapsed the sidebar with the PR inspector open and verified the conversation, panel, rail, header, and composer all remained visible.
- Restored the sidebar and dismissed the active inspector tool.
- Verified no Changes, Terminal, or PR tabs remain in the top-right session header.
- Verified the titlebar separator is absent and the content background continues behind the macOS traffic lights.
- Added component coverage for complete sidebar removal, one-column collapsed layout, inspector placement, active-state toggling, and panel dismissal.

## Final result

passed

---

# Project sidebar controls extension QA

## Evidence

- Source visual truth: `/var/folders/2h/k_vpn3_n7kl2knmtx_w78s940000gn/T/codex-clipboard-e228fb5d-4ea4-422b-b733-ce3c436c4c18.png` (expanded organization/sort menu) and `/var/folders/2h/k_vpn3_n7kl2knmtx_w78s940000gn/T/codex-clipboard-d17d3c8c-6e3c-4de6-93ba-e6f9029549d7.png` (collapsed header controls).
- Rendered implementation: `../outputs/openade-project-sidebar.png` (grouped projects) and `../outputs/openade-project-sidebar-flat.png` (one-list chats), both 1,194 × 768 px native Wails windows in the Glass theme.
- Combined comparison: `../outputs/openade-project-settings-comparison.png`; the 665 × 788 px source and a same-width native-app crop are shown without horizontal scaling.
- State: Projects expanded, priority sort applied, grouped project hierarchy visible, ellipsis and add-project actions idle. The open menu was separately verified in the macOS accessibility tree with all five radio choices and their checked states.

## Findings

- P0: none.
- P1: none.
- P2: none.
- P3: the reference is a warm Dusk crop with no primary navigation, while OpenADE retains its active Glass shell and existing navigation above Projects. Control order, menu dimensions, copy, check placement, and hierarchy match within the established shell.

## Required fidelity surfaces

- Fonts and typography: existing Inter tokens are retained; menu group labels are quieter than choices, selected values remain readable, and project/chat names truncate without colliding with the caret or actions.
- Spacing and layout rhythm: the header holds Projects, disclosure, ellipsis, and plus on one baseline. The 174 px menu uses compact 28 px rows, a 12 px radius, and reserved check space so labels never shift when a choice changes.
- Colors and tokens: all surfaces, borders, muted labels, hover states, and Glass blur reuse OpenADE theme variables. Paper, Graphite, Dusk, System, and Glass require no menu-specific override.
- Image quality and assets: disclosure, check, folder, ellipsis, and plus use the existing Phosphor icon library. No placeholder asset, handcrafted SVG, or CSS-drawn icon was introduced.
- Copy and content: “Organize sidebar,” “By project,” “In one list,” “Sort chats by,” “Priority,” “Last updated,” and “Manual order” reproduce the reference vocabulary.

## Interactions tested

- Opened and dismissed the project menu; outside-pointer and Escape dismissal are supported.
- Switched between grouped projects and a flat chat list; the choice persists in local preferences.
- Switched among priority, last-updated, and manual ordering; the choice persists and project ordering updates immediately.
- Kept Recents chronological regardless of the project-section sort choice.
- Collapsed and expanded the Projects section from its disclosure control.
- Verified the plus action routes to Settings, where the workspace folder can be selected and scanned.
- Checked the native macOS accessibility tree for the disclosure, menu trigger, add-project action, menu group labels, and checked radio states.

## Final result

passed

---

# Sites empty-state extension QA

## Evidence

- Source visual truth: `/var/folders/2h/k_vpn3_n7kl2knmtx_w78s940000gn/T/codex-clipboard-5c7371e4-4fb5-4d89-9c38-7e29f1c485c7.png` (2,559 × 1,038 px ChatGPT Sites empty state).
- Rendered implementation: `../outputs/openade-sites-empty.png` (1,194 × 768 px Wails window, Glass theme).
- Full-view comparison: `../outputs/openade-sites-full-comparison.png`.
- Focused comparison: `../outputs/openade-sites-focused-comparison.png`; the 1,307 × 1,038 source content crop was normalized to the implementation main-content crop at 968 × 768 px. The source came from a wider, higher-density desktop capture, so this normalized comparison is used for content alignment rather than raw full-window pixel equivalence.
- State: Sites selected in primary navigation, empty collection, idle search field, Create and refresh affordances visible.

## Findings

- P0: none.
- P1: none.
- P2: none after correction.
- P3: OpenADE keeps the active Glass palette and its existing 280 px project sidebar instead of copying the source application's warm Dusk palette and narrower navigation. This is intentional theme and product-shell continuity.

## Required fidelity surfaces

- Fonts and typography: Inter, sentence case, compact 25 px page title, 12 px subtitle, and restrained empty-state hierarchy match the reference while using OpenADE's established type tokens.
- Spacing and layout rhythm: centered 700 px header/search column, pill search field, top-right actions, and the empty-state group align with the normalized source crop. Persistent controls remain visible at 1,194 × 768.
- Colors and tokens: the same surface, line, text, muted, accent, and Glass blur tokens used elsewhere in OpenADE are applied; no page-specific palette was introduced.
- Image quality and assets: the source contains no raster imagery. All visible icons use the existing Phosphor library; no placeholder art, handcrafted SVG, or CSS drawing was introduced.
- Copy and content: “Sites,” “Turn your ideas into live websites,” “Search sites,” “No sites yet,” and both Create labels reproduce the reference interaction vocabulary.

## Comparison history

1. Initial implementation matched navigation, header, search, actions, icon, copy, and empty CTA, but the empty-state group sat roughly 75 px below the normalized source position and the refresh icon had an unnecessary bordered container (P2).
2. Final correction moved the empty-state anchor from 48% to 39% and removed the refresh border. The revised focused comparison shows aligned header/search and empty-state composition with no actionable P0/P1/P2 mismatch.

## Interactions tested

- Opened Sites from the primary sidebar and verified selected navigation state.
- Typed into the local presentation-only search field through automated component coverage.
- Verified both Create hooks and the refresh hook are exposed as UI affordances without backend behavior.
- Checked the macOS accessibility tree for page title, search field, empty-state label, refresh, and both Create actions.

## Final result

passed

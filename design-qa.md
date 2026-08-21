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

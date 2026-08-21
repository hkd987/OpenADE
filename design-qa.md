# Glass theme design QA

## Evidence

- Source visual truth: `../outputs/openade-home-refined.png` and matched live Graphite captures in `../work/glass-theme-qa/`
- Implementation: `../work/glass-theme-qa/03-home-glass.png`, `04-chat-glass.png`, `05-changes-glass.png`, and `08-terminal-glass-matched.png`
- Viewport and CSS size: 1194 × 768 px macOS Wails window
- Source and implementation density: matching 1194 × 768 pixel captures; no density normalization required
- State: real indexed sessions, completed Codex chat, open Changes panel, and two attached project terminals
- Full-view comparisons: `home-comparison.png`, `chat-comparison.png`, and `terminal-comparison.png` in `../work/glass-theme-qa/`
- Focused comparison: the matched terminal comparison verifies that terminal content remains opaque and crisp while surrounding structural surfaces use Glass tokens. No additional crop was needed because the terminal occupies the full right half of the matched viewport.

## Fidelity review

- Typography: unchanged OpenADE Inter hierarchy, weights, wrapping, and utility sizing; contrast remains clear on the cool translucent surfaces.
- Spacing and layout: no grid, padding, radius, or panel-width drift from Graphite. Glass adds depth without moving controls.
- Colors and tokens: cool navy base, restrained blue ambient light, translucent structural surfaces, pale edge highlights, and preserved semantic status colors.
- Image quality and assets: no new raster or decorative assets; existing Phosphor icons remain sharp and consistent.
- Copy and content: Glass is labeled “Cool translucent workspace”; all app-specific copy and control names remain unchanged.
- Terminal and diff safety: terminal and diff canvases retain opaque near-black backgrounds for predictable code contrast.

## Findings

- P0: none.
- P1: none.
- P2: none.
- P3: native macOS behind-window translucency is not enabled; this theme intentionally creates depth within the existing Wails content surface so behavior remains portable and predictable.

## Comparison history

- Initial comparison: passed with no actionable P0/P1/P2 differences. The layout and interaction model match Graphite, while the requested visual change is isolated to theme tokens and material treatment.

## Interactions tested

- Selected Glass from Settings and verified local preference persistence.
- Opened Home, resumed chat, Changes, and Terminal surfaces.
- Verified two live terminal tabs remained attached and readable after theme switching.
- Switched Graphite → Glass → Graphite → Glass without restarting the daemon or losing session state.

## Final result

passed

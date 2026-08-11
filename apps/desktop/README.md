# OpenADE desktop app

Vite + React + TypeScript UI in a Tauri 2 shell. The UI is a *viewer*: all
session state lives in `openade-daemon` (see `crates/openade-daemon`), which
the app talks to at `http://127.0.0.1:7433` (override with
`VITE_OPENADE_DAEMON_URL`). Closing the window never kills a session.

## Develop the UI only (no system dependencies needed)

```sh
# terminal 1: the daemon
cargo run -p openade-daemon

# terminal 2: the UI in a browser
cd apps/desktop
npm install
npm run dev
```

`npm run build` type-checks and produces `dist/` — this is what CI verifies.

## Run the native shell

The `src-tauri` crate is **excluded from the root Cargo workspace** because it
needs platform webview libraries:

- **macOS**: Xcode command-line tools.
- **Linux**: `libwebkit2gtk-4.1-dev`, `build-essential`, `libssl-dev`,
  `libayatana-appindicator3-dev`, `librsvg2-dev` (Debian/Ubuntu names).

```sh
cd apps/desktop
npm install
npx @tauri-apps/cli dev     # or: cargo tauri dev
```

App icons are not checked in yet (`bundle.active` is `false`); generate them
with `npx @tauri-apps/cli icon <source.png>` before enabling bundling.

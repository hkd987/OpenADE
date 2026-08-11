//! OpenADE desktop shell.
//!
//! The shell is intentionally thin: all session state lives in
//! `openade-daemon`, which the webview talks to over its localhost API, so
//! sessions survive the window closing. The shell's future responsibilities
//! are daemon lifecycle (spawn on first launch, health checks) and native
//! niceties (menus, notifications on needs-input).

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running OpenADE");
}

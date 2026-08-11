//! `openade-server` — the self-hostable multiplayer workspace server.
//!
//! Boot: set `OPENADE_SERVER_ADMIN_TOKEN`, run, mint member tokens with
//! `POST /tokens` (admin token), hand each teammate their token. See
//! docs/multiplayer.md.

use std::sync::Arc;

use openade_server::server::{router, AppState};
use openade_server::store::Store;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let data_dir = std::env::var("OPENADE_SERVER_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            dirs_fallback_home().join(".openade-server")
        });
    let admin_token = std::env::var("OPENADE_SERVER_ADMIN_TOKEN").unwrap_or_default();
    if admin_token.is_empty() {
        tracing::warn!(
            "OPENADE_SERVER_ADMIN_TOKEN is not set — no tokens can be minted; \
             set it and restart to administer this server"
        );
    }
    let port: u16 = std::env::var("OPENADE_SERVER_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(7500);
    let bind = std::env::var("OPENADE_SERVER_BIND").unwrap_or_else(|_| "0.0.0.0".into());

    let store = Store::open(&data_dir)?;
    let app = router(Arc::new(AppState { store, admin_token }));

    let addr: std::net::SocketAddr = format!("{bind}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(
        "openade-server listening on http://{addr} (data dir: {})",
        data_dir.display()
    );
    axum::serve(listener, app).await?;
    Ok(())
}

fn dirs_fallback_home() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

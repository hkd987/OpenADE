//! `openade-daemon` — the session host process.
//!
//! Binds the session API to loopback. The desktop app (or curl) attaches to
//! it; sessions keep running when the UI goes away.

use std::sync::Arc;

use openade_daemon::server;
use openade_daemon::Daemon;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let data_dir = openade_daemon::transcript::default_data_dir();
    let port: u16 = std::env::var("OPENADE_DAEMON_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(7433);

    let daemon = Arc::new(Daemon::open(&data_dir)?);
    let app = server::router(daemon);

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(
        "openade-daemon listening on http://{addr} (data dir: {})",
        data_dir.display()
    );
    axum::serve(listener, app).await?;
    Ok(())
}

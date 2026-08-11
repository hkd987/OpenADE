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

    let mut daemon = Daemon::open(&data_dir)?;
    // Configured memory sources mean entity-launched sessions get context
    // bundles + catalog MCP registration; without any the daemon is a plain
    // (still fully functional) session manager. Sources: Backstage
    // (BACKSTAGE_BASE_URL) and/or GitHub via the user's local gh CLI.
    match catalog_mcp::MemoryRouter::from_env() {
        Some(router) => {
            let sources = router.source_names();
            tracing::info!("memory sources: {}", sources.join(", "));
            // A logged-out gh fails every GitHub memory call later — say so
            // once at startup, with the fix.
            if sources.contains(&"github") {
                if let Some(gh) = catalog_mcp::github::resolve_gh_bin() {
                    if let Some(warning) = catalog_mcp::github::gh_auth_warning(&gh) {
                        tracing::warn!("{warning}");
                    }
                }
            }
            daemon = daemon.with_catalog(Arc::new(router));
        }
        None => {
            tracing::info!(
                "memory sources: none — set BACKSTAGE_BASE_URL for Backstage, and/or {} \
                 for GitHub memory",
                catalog_mcp::github::GH_SETUP_HINT
            );
        }
    }
    // Shared team memory: one repo everyone writes to, straight to its
    // default branch (OPENADE_MEMORY_REPO=owner/name, pushed via the local
    // gh CLI). Session knowledge lands there immediately for the whole team.
    if let Some(memory_repo) = openade_daemon::memory_repo::MemoryRepo::from_env() {
        tracing::info!("shared memory repo: {}", memory_repo.repo());
        daemon = daemon.with_memory_repo(memory_repo);
    }
    let daemon = Arc::new(daemon);
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

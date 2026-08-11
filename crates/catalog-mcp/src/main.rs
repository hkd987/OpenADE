//! `catalog-mcp` — MCP server binary (stdio transport).
//!
//! Memory sources are configured via environment:
//! - Backstage: `BACKSTAGE_BASE_URL` (+ optional `BACKSTAGE_TOKEN`)
//! - GitHub: the user's local, `gh auth login`-authenticated `gh` CLI
//!   (`OPENADE_GH_BIN` overrides the binary; `OPENADE_GITHUB_MEMORY=0`
//!   disables the source)
//!
//! Entity refs route by kind: `repo:owner/name` → GitHub, everything else →
//! Backstage. Harnesses spawn this binary per session; see the adapter layer
//! in `openade-daemon` for how it gets registered with each CLI.

use catalog_mcp::mcp::McpServer;
use catalog_mcp::MemoryRouter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Logs go to stderr; stdout belongs to the MCP protocol.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let router = MemoryRouter::from_env().ok_or_else(|| {
        anyhow::anyhow!(
            "no memory source configured: set BACKSTAGE_BASE_URL for Backstage, and/or {} \
             for GitHub repo context",
            catalog_mcp::github::GH_SETUP_HINT
        )
    })?;
    let sources = router.source_names();
    tracing::info!("catalog-mcp memory sources: {}", sources.join(", "));
    if sources.contains(&"github") {
        if let Some(gh) = catalog_mcp::github::resolve_gh_bin() {
            if let Some(warning) = catalog_mcp::github::gh_auth_warning(&gh) {
                tracing::warn!("{warning}");
            }
        }
    }

    let server = McpServer::new(router);
    server
        .serve(tokio::io::stdin(), tokio::io::stdout())
        .await?;
    Ok(())
}

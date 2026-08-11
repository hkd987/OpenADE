//! `catalog-mcp` — MCP server binary (stdio transport).
//!
//! Configuration via environment:
//! - `BACKSTAGE_BASE_URL` (required): e.g. `https://backstage.example.com`
//! - `BACKSTAGE_TOKEN` (optional): static service-to-service bearer token
//!
//! Harnesses spawn this binary per session; see the adapter layer in
//! `openade-daemon` for how it gets registered with each CLI.

use catalog_mcp::backstage::{BackstageConfig, BackstageProvider};
use catalog_mcp::mcp::McpServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Logs go to stderr; stdout belongs to the MCP protocol.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = BackstageConfig::from_env().map_err(|e| anyhow::anyhow!(e))?;
    tracing::info!("catalog-mcp serving Backstage at {}", config.base_url);

    let server = McpServer::new(BackstageProvider::new(config));
    server
        .serve(tokio::io::stdin(), tokio::io::stdout())
        .await?;
    Ok(())
}

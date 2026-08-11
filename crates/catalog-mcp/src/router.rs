//! Composes memory sources so Backstage and GitHub coexist behind one
//! [`CatalogProvider`].
//!
//! Routing is by entity kind: `repo:` refs go to the GitHub source (the
//! user's `gh` CLI), everything else to the catalog (Backstage). Search fans
//! out to every source and concatenates within the limit.

use std::sync::Arc;

use async_trait::async_trait;

use crate::backstage::{BackstageConfig, BackstageProvider};
use crate::github::{GithubProvider, REPO_KIND};
use crate::provider::{CatalogProvider, Entity, EntityRef, ProviderError};

/// One routed source: the kinds it serves (empty = fallback for all kinds)
/// plus a display name for logs.
struct Source {
    name: &'static str,
    kinds: &'static [&'static str],
    provider: Arc<dyn CatalogProvider>,
}

/// Routes entity refs to the memory source that serves their kind.
pub struct MemoryRouter {
    sources: Vec<Source>,
}

impl MemoryRouter {
    /// Build from explicit sources (either may be absent).
    pub fn new(
        backstage: Option<Arc<dyn CatalogProvider>>,
        github: Option<Arc<dyn CatalogProvider>>,
    ) -> Option<Self> {
        let mut sources = Vec::new();
        if let Some(provider) = github {
            sources.push(Source {
                name: "github",
                kinds: &[REPO_KIND],
                provider,
            });
        }
        if let Some(provider) = backstage {
            // Fallback: serves every kind the more specific sources don't.
            sources.push(Source {
                name: "backstage",
                kinds: &[],
                provider,
            });
        }
        if sources.is_empty() {
            None
        } else {
            Some(MemoryRouter { sources })
        }
    }

    /// Build whichever sources the environment enables:
    /// - Backstage via `BACKSTAGE_BASE_URL` (+ optional `BACKSTAGE_TOKEN`)
    /// - GitHub via the local `gh` CLI (`OPENADE_GH_BIN` override,
    ///   `OPENADE_GITHUB_MEMORY=0` to disable)
    ///
    /// Returns `None` when no source is configured.
    pub fn from_env() -> Option<Self> {
        let backstage = match BackstageConfig::from_env() {
            Ok(config) => {
                tracing::info!("memory source: Backstage at {}", config.base_url);
                Some(Arc::new(BackstageProvider::new(config)) as Arc<dyn CatalogProvider>)
            }
            Err(reason) => {
                tracing::debug!("backstage memory source not configured: {reason}");
                None
            }
        };
        let github = match GithubProvider::from_env() {
            Ok(provider) => {
                tracing::info!("memory source: GitHub via the local gh CLI");
                Some(Arc::new(provider) as Arc<dyn CatalogProvider>)
            }
            Err(reason) => {
                tracing::debug!("github memory source not configured: {reason}");
                None
            }
        };
        MemoryRouter::new(backstage, github)
    }

    /// Names of the active sources (for startup logs).
    pub fn source_names(&self) -> Vec<&'static str> {
        self.sources.iter().map(|s| s.name).collect()
    }

    fn route(&self, kind: &str) -> Option<&Source> {
        self.sources
            .iter()
            .find(|s| s.kinds.contains(&kind))
            .or_else(|| self.sources.iter().find(|s| s.kinds.is_empty()))
    }

    fn provider_for(
        &self,
        entity_ref: &EntityRef,
    ) -> Result<&Arc<dyn CatalogProvider>, ProviderError> {
        self.route(&entity_ref.kind)
            .map(|s| &s.provider)
            .ok_or_else(|| {
                ProviderError::NotFound(format!(
                    "{entity_ref} (no memory source serves kind {:?}; active: {})",
                    entity_ref.kind,
                    self.source_names().join(", ")
                ))
            })
    }
}

#[async_trait]
impl CatalogProvider for MemoryRouter {
    async fn get_entity(&self, entity_ref: &EntityRef) -> Result<Entity, ProviderError> {
        self.provider_for(entity_ref)?.get_entity(entity_ref).await
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Entity>, ProviderError> {
        // Fan out to every source; a source erroring must not hide results
        // from the others — but if every source fails, surface the first
        // error rather than an empty success.
        let mut results = Vec::new();
        let mut first_error: Option<ProviderError> = None;
        for source in &self.sources {
            match source.provider.search(query, limit).await {
                Ok(mut items) => results.append(&mut items),
                Err(e) => {
                    tracing::warn!("memory source {} search failed: {e}", source.name);
                    first_error.get_or_insert(e);
                }
            }
        }
        if results.is_empty() {
            if let Some(e) = first_error {
                return Err(e);
            }
        }
        results.truncate(limit);
        Ok(results)
    }

    async fn get_techdocs_page(
        &self,
        entity_ref: &EntityRef,
        page_path: &str,
    ) -> Result<String, ProviderError> {
        self.provider_for(entity_ref)?
            .get_techdocs_page(entity_ref, page_path)
            .await
    }
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;

//! Backstage implementation of [`CatalogProvider`] (PRD §7.3).
//!
//! Read-only against three Backstage surfaces:
//! - Catalog REST API: `/api/catalog/entities/by-name/...`, `/entities/by-query`
//! - TechDocs static content: `/api/techdocs/static/docs/...`
//!
//! Auth is a static service-to-service bearer token (v0); OAuth flows are
//! documented for orgs that need them (PRD §7.3 "Auth").

use async_trait::async_trait;
use reqwest::StatusCode;
use serde::Deserialize;

use crate::provider::{CatalogProvider, Entity, EntityRef, ProviderError};

/// Configuration for a Backstage connection.
#[derive(Debug, Clone)]
pub struct BackstageConfig {
    /// Base URL of the Backstage backend, e.g. `https://backstage.example.com`.
    pub base_url: String,
    /// Optional static bearer token.
    pub token: Option<String>,
}

impl BackstageConfig {
    /// Read configuration from `BACKSTAGE_BASE_URL` / `BACKSTAGE_TOKEN`.
    pub fn from_env() -> Result<Self, String> {
        let base_url = std::env::var("BACKSTAGE_BASE_URL")
            .map_err(|_| "BACKSTAGE_BASE_URL is not set".to_string())?;
        let token = std::env::var("BACKSTAGE_TOKEN")
            .ok()
            .filter(|t| !t.is_empty());
        Ok(BackstageConfig { base_url, token })
    }
}

/// [`CatalogProvider`] backed by the Backstage REST APIs.
pub struct BackstageProvider {
    config: BackstageConfig,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct ByQueryResponse {
    #[serde(default)]
    items: Vec<Entity>,
}

impl BackstageProvider {
    pub fn new(config: BackstageConfig) -> Self {
        BackstageProvider {
            config,
            client: reqwest::Client::new(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.config.base_url.trim_end_matches('/'))
    }

    fn request(&self, url: &str) -> reqwest::RequestBuilder {
        let mut req = self.client.get(url);
        if let Some(token) = &self.config.token {
            req = req.bearer_auth(token);
        }
        req
    }

    async fn get(&self, url: String, not_found: &str) -> Result<reqwest::Response, ProviderError> {
        let response = self
            .request(&url)
            .send()
            .await
            .map_err(|e| ProviderError::Transport(e.to_string()))?;
        match response.status() {
            StatusCode::NOT_FOUND => Err(ProviderError::NotFound(not_found.to_string())),
            status if !status.is_success() => Err(ProviderError::Upstream {
                status: status.as_u16(),
                message: response.text().await.unwrap_or_default(),
            }),
            _ => Ok(response),
        }
    }
}

#[async_trait]
impl CatalogProvider for BackstageProvider {
    async fn get_entity(&self, entity_ref: &EntityRef) -> Result<Entity, ProviderError> {
        let url = self.url(&format!(
            "/api/catalog/entities/by-name/{}/{}/{}",
            entity_ref.kind, entity_ref.namespace, entity_ref.name
        ));
        let response = self.get(url, &entity_ref.to_string()).await?;
        response
            .json()
            .await
            .map_err(|e| ProviderError::Transport(e.to_string()))
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Entity>, ProviderError> {
        // Catalog by-query with full-text filter — a deliberate v0 choice
        // over the Search API plugin: it needs no extra backend plugins and
        // returns full entities. Revisit if orgs want ranked TechDocs search.
        let url = self.url("/api/catalog/entities/by-query");
        let response = self
            .request(&url)
            .query(&[("fullTextFilter", query), ("limit", &limit.to_string())])
            .send()
            .await
            .map_err(|e| ProviderError::Transport(e.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(ProviderError::Upstream {
                status: status.as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }
        let parsed: ByQueryResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::Transport(e.to_string()))?;
        Ok(parsed.items)
    }

    async fn get_techdocs_page(
        &self,
        entity_ref: &EntityRef,
        page_path: &str,
    ) -> Result<String, ProviderError> {
        let page = page_path.trim_start_matches('/');
        let url = self.url(&format!(
            "/api/techdocs/static/docs/{}/{}/{}/{page}",
            entity_ref.namespace, entity_ref.kind, entity_ref.name
        ));
        let response = self
            .get(url, &format!("techdocs page {page} for {entity_ref}"))
            .await?;
        response
            .text()
            .await
            .map_err(|e| ProviderError::Transport(e.to_string()))
    }
}

#[cfg(test)]
#[path = "backstage_tests.rs"]
mod tests;

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
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn payments_entity_json() -> serde_json::Value {
        serde_json::json!({
            "apiVersion": "backstage.io/v1alpha1",
            "kind": "Component",
            "metadata": {
                "name": "payments-api",
                "namespace": "default",
                "title": "Payments API",
                "description": "Handles payments."
            },
            "spec": {"type": "service", "lifecycle": "production"},
            "relations": [
                {"type": "ownedBy", "targetRef": "group:default/payments-team"},
                {"type": "dependsOn", "targetRef": "component:default/ledger"},
                {"type": "providesApi", "targetRef": "api:default/payments-v2"}
            ]
        })
    }

    async fn provider(server: &MockServer, token: Option<&str>) -> BackstageProvider {
        BackstageProvider::new(BackstageConfig {
            base_url: server.uri(),
            token: token.map(str::to_string),
        })
    }

    #[tokio::test]
    async fn fetches_entity_by_name_with_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/api/catalog/entities/by-name/component/default/payments-api",
            ))
            .and(header("authorization", "Bearer sekrit"))
            .respond_with(ResponseTemplate::new(200).set_body_json(payments_entity_json()))
            .mount(&server)
            .await;

        let p = provider(&server, Some("sekrit")).await;
        let entity = p
            .get_entity(&"component:default/payments-api".parse().unwrap())
            .await
            .unwrap();
        assert_eq!(entity.display_title(), "Payments API");
        assert_eq!(
            entity.relation_targets("dependsOn"),
            vec!["component:default/ledger"]
        );
    }

    #[tokio::test]
    async fn missing_entity_maps_to_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let p = provider(&server, None).await;
        let err = p
            .get_entity(&"component:default/ghost".parse().unwrap())
            .await
            .unwrap_err();
        assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
    }

    #[tokio::test]
    async fn upstream_errors_carry_status() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;
        let p = provider(&server, None).await;
        let err = p
            .get_entity(&"component:default/x".parse().unwrap())
            .await
            .unwrap_err();
        match err {
            ProviderError::Upstream { status, message } => {
                assert_eq!(status, 500);
                assert_eq!(message, "boom");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn search_uses_full_text_filter() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/catalog/entities/by-query"))
            .and(query_param("fullTextFilter", "payments"))
            .and(query_param("limit", "5"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [payments_entity_json()]
            })))
            .mount(&server)
            .await;
        let p = provider(&server, None).await;
        let hits = p.search("payments", 5).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].metadata.name, "payments-api");
    }

    #[tokio::test]
    async fn fetches_techdocs_pages() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/api/techdocs/static/docs/default/component/payments-api/adr-007.md",
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("# ADR-007\nUse idempotency keys."),
            )
            .mount(&server)
            .await;
        let p = provider(&server, None).await;
        let page = p
            .get_techdocs_page(
                &"component:default/payments-api".parse().unwrap(),
                "/adr-007.md",
            )
            .await
            .unwrap();
        assert!(page.contains("idempotency keys"));
    }

    #[tokio::test]
    async fn default_trait_helpers_walk_relations() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/api/catalog/entities/by-name/component/default/payments-api",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(payments_entity_json()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(
                "/api/catalog/entities/by-name/group/default/payments-team",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "apiVersion": "backstage.io/v1alpha1",
                "kind": "Group",
                "metadata": {"name": "payments-team", "title": "Payments Team"},
                "spec": {"type": "team"}
            })))
            .mount(&server)
            .await;
        // ledger + payments-v2 unresolvable → skipped/unresolved, not fatal.
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let p = provider(&server, None).await;
        let entity = p
            .get_entity(&"component:default/payments-api".parse().unwrap())
            .await
            .unwrap();

        let owners = p.owners_of(&entity).await.unwrap();
        assert_eq!(owners.len(), 1);
        assert_eq!(owners[0].display_title(), "Payments Team");

        let deps = p.dependencies_of(&entity).await.unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].1, "component:default/ledger");
        assert!(
            deps[0].2.is_none(),
            "unresolvable dep is kept as a bare ref"
        );

        let apis = p.apis_of(&entity).await.unwrap();
        assert_eq!(apis.len(), 1);
        assert_eq!(apis[0].0, "providesApi");
    }
}

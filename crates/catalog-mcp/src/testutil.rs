//! In-memory [`CatalogProvider`] used by unit tests across the crate.

/// Serializes tests that mutate process-global environment variables
/// (`BACKSTAGE_*`, `OPENADE_GH_BIN`, `OPENADE_GITHUB_MEMORY`). Lock it in
/// every test that touches env config; `lock().unwrap_or_else` tolerates
/// poisoning from a failed test.
pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

use std::collections::HashMap;

use async_trait::async_trait;

use crate::provider::{
    CatalogProvider, Entity, EntityLink, EntityMetadata, EntityRef, ProviderError, Relation,
};

/// A small fixed catalog: payments-api → (ownedBy payments-team,
/// dependsOn ledger, providesApi payments-v2 [unresolvable]).
pub struct MockProvider {
    entities: HashMap<String, Entity>,
}

impl MockProvider {
    pub fn with_payments_graph() -> Self {
        let mut entities = HashMap::new();
        entities.insert(
            "component:default/payments-api".into(),
            Entity {
                api_version: "backstage.io/v1alpha1".into(),
                kind: "Component".into(),
                metadata: EntityMetadata {
                    name: "payments-api".into(),
                    namespace: Some("default".into()),
                    title: Some("Payments API".into()),
                    description: Some("Handles payments.".into()),
                    tags: vec!["tier-1".into()],
                    links: vec![EntityLink {
                        url: "https://example.com/runbook".into(),
                        title: Some("Runbook".into()),
                    }],
                    ..Default::default()
                },
                spec: serde_json::json!({"type": "service", "lifecycle": "production"}),
                relations: vec![
                    Relation {
                        relation_type: "ownedBy".into(),
                        target_ref: "group:default/payments-team".into(),
                    },
                    Relation {
                        relation_type: "dependsOn".into(),
                        target_ref: "component:default/ledger".into(),
                    },
                    Relation {
                        relation_type: "providesApi".into(),
                        target_ref: "api:default/payments-v2".into(),
                    },
                ],
            },
        );
        entities.insert(
            "group:default/payments-team".into(),
            Entity {
                api_version: "backstage.io/v1alpha1".into(),
                kind: "Group".into(),
                metadata: EntityMetadata {
                    name: "payments-team".into(),
                    namespace: Some("default".into()),
                    title: Some("Payments Team".into()),
                    links: vec![EntityLink {
                        url: "https://chat.example.com/payments-eng".into(),
                        title: None,
                    }],
                    ..Default::default()
                },
                spec: serde_json::json!({"type": "team"}),
                relations: vec![],
            },
        );
        entities.insert(
            "component:default/ledger".into(),
            Entity {
                api_version: "backstage.io/v1alpha1".into(),
                kind: "Component".into(),
                metadata: EntityMetadata {
                    name: "ledger".into(),
                    namespace: Some("default".into()),
                    description: Some("Double-entry ledger.".into()),
                    ..Default::default()
                },
                spec: serde_json::json!({"type": "service"}),
                relations: vec![],
            },
        );
        // A GitHub-shaped repo entity, as the `gh`-backed source would build
        // it (CODEOWNERS-derived team + user ownership).
        entities.insert(
            "repo:acme/payments-service".into(),
            Entity {
                api_version: "openade.dev/github-v1".into(),
                kind: "repo".into(),
                metadata: EntityMetadata {
                    name: "payments-service".into(),
                    namespace: Some("acme".into()),
                    title: Some("acme/payments-service".into()),
                    description: Some("Payments service repository on GitHub.".into()),
                    tags: vec!["payments".into()],
                    links: vec![EntityLink {
                        url: "https://github.com/acme/payments-service".into(),
                        title: Some("Repository".into()),
                    }],
                    ..Default::default()
                },
                spec: serde_json::json!({"type": "Rust", "lifecycle": "active", "default_branch": "main"}),
                relations: vec![
                    Relation {
                        relation_type: "ownedBy".into(),
                        target_ref: "group:acme/payments-team".into(),
                    },
                    Relation {
                        relation_type: "ownedBy".into(),
                        target_ref: "user:github/lundin".into(),
                    },
                ],
            },
        );
        MockProvider { entities }
    }
}

#[async_trait]
impl CatalogProvider for MockProvider {
    async fn get_entity(&self, entity_ref: &EntityRef) -> Result<Entity, ProviderError> {
        self.entities
            .get(&entity_ref.to_string())
            .cloned()
            .ok_or_else(|| ProviderError::NotFound(entity_ref.to_string()))
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Entity>, ProviderError> {
        Ok(self
            .entities
            .values()
            .filter(|e| {
                e.metadata.name.contains(query)
                    || e.metadata
                        .description
                        .as_deref()
                        .unwrap_or("")
                        .contains(query)
            })
            .take(limit)
            .cloned()
            .collect())
    }

    async fn get_techdocs_page(
        &self,
        entity_ref: &EntityRef,
        page_path: &str,
    ) -> Result<String, ProviderError> {
        Ok(format!("# docs for {entity_ref} at {page_path}"))
    }
}

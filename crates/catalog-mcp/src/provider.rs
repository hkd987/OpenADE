//! The catalog backend abstraction (PRD G4: swap Backstage for another IDP
//! behind a stable provider interface).

use std::fmt;
use std::str::FromStr;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A parsed entity reference: `kind:namespace/name` (namespace defaults to
/// `default`, matching Backstage semantics).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntityRef {
    pub kind: String,
    pub namespace: String,
    pub name: String,
}

impl EntityRef {
    pub fn new(kind: &str, namespace: &str, name: &str) -> Self {
        EntityRef {
            kind: kind.to_lowercase(),
            namespace: namespace.to_lowercase(),
            name: name.to_string(),
        }
    }
}

impl fmt::Display for EntityRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}/{}", self.kind, self.namespace, self.name)
    }
}

/// Error for malformed entity references.
#[derive(Debug, Clone, thiserror::Error)]
#[error(
    "invalid entity ref {0:?} (expected kind:namespace/name, e.g. component:default/payments-api)"
)]
pub struct InvalidEntityRef(pub String);

impl FromStr for EntityRef {
    type Err = InvalidEntityRef;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (kind, rest) = s
            .split_once(':')
            .ok_or_else(|| InvalidEntityRef(s.to_string()))?;
        let (namespace, name) = match rest.split_once('/') {
            Some((ns, name)) => (ns, name),
            None => ("default", rest),
        };
        if kind.is_empty() || namespace.is_empty() || name.is_empty() {
            return Err(InvalidEntityRef(s.to_string()));
        }
        Ok(EntityRef::new(kind, namespace, name))
    }
}

/// A catalog entity in (Backstage-shaped) neutral form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    #[serde(rename = "apiVersion", default)]
    pub api_version: String,
    pub kind: String,
    pub metadata: EntityMetadata,
    #[serde(default)]
    pub spec: serde_json::Value,
    #[serde(default)]
    pub relations: Vec<Relation>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EntityMetadata {
    pub name: String,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub annotations: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub links: Vec<EntityLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityLink {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
}

/// A relation edge from the catalog graph (`dependsOn`, `ownedBy`,
/// `providesApi`, ...).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    #[serde(rename = "type")]
    pub relation_type: String,
    #[serde(rename = "targetRef")]
    pub target_ref: String,
}

impl Entity {
    /// The entity's full reference.
    pub fn entity_ref(&self) -> EntityRef {
        EntityRef::new(
            &self.kind,
            self.metadata.namespace.as_deref().unwrap_or("default"),
            &self.metadata.name,
        )
    }

    /// Target refs of relations of the given type.
    pub fn relation_targets(&self, relation_type: &str) -> Vec<&str> {
        self.relations
            .iter()
            .filter(|r| r.relation_type == relation_type)
            .map(|r| r.target_ref.as_str())
            .collect()
    }

    /// Display title, falling back to the name.
    pub fn display_title(&self) -> &str {
        self.metadata
            .title
            .as_deref()
            .unwrap_or(&self.metadata.name)
    }
}

/// Errors a provider can surface. Kept small and backend-neutral so tool
/// responses stay stable across providers.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("entity not found: {0}")]
    NotFound(String),
    #[error("catalog request failed with status {status}: {message}")]
    Upstream { status: u16, message: String },
    #[error("transport error: {0}")]
    Transport(String),
    #[error(transparent)]
    InvalidRef(#[from] InvalidEntityRef),
}

/// Read interface every catalog backend implements.
///
/// Only three primitives are required; the relation-walking helpers
/// (`owners_of`, `dependencies_of`, `apis_of`) have default implementations
/// on top of [`CatalogProvider::get_entity`].
#[async_trait]
pub trait CatalogProvider: Send + Sync {
    /// Fetch one entity by reference.
    async fn get_entity(&self, entity_ref: &EntityRef) -> Result<Entity, ProviderError>;

    /// Full-text search over the catalog.
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Entity>, ProviderError>;

    /// Fetch a TechDocs page for an entity (markdown or HTML as published).
    async fn get_techdocs_page(
        &self,
        entity_ref: &EntityRef,
        page_path: &str,
    ) -> Result<String, ProviderError>;

    /// Entities that own `entity` (via `ownedBy` relations). Unresolvable
    /// owner refs are skipped rather than failing the whole call.
    async fn owners_of(&self, entity: &Entity) -> Result<Vec<Entity>, ProviderError> {
        let mut owners = Vec::new();
        for target in entity.relation_targets("ownedBy") {
            let entity_ref: EntityRef = target.parse()?;
            if let Ok(owner) = self.get_entity(&entity_ref).await {
                owners.push(owner);
            }
        }
        Ok(owners)
    }

    /// Relation edges (type + target entity when resolvable) for the given
    /// relation types.
    async fn related_of(
        &self,
        entity: &Entity,
        relation_types: &[&str],
    ) -> Result<Vec<(String, String, Option<Entity>)>, ProviderError> {
        let mut out = Vec::new();
        for rel in &entity.relations {
            if !relation_types.contains(&rel.relation_type.as_str()) {
                continue;
            }
            let resolved = match rel.target_ref.parse::<EntityRef>() {
                Ok(r) => self.get_entity(&r).await.ok(),
                Err(_) => None,
            };
            out.push((rel.relation_type.clone(), rel.target_ref.clone(), resolved));
        }
        Ok(out)
    }

    /// What the entity depends on (`dependsOn`).
    async fn dependencies_of(
        &self,
        entity: &Entity,
    ) -> Result<Vec<(String, String, Option<Entity>)>, ProviderError> {
        self.related_of(entity, &["dependsOn"]).await
    }

    /// APIs the entity provides or consumes.
    async fn apis_of(
        &self,
        entity: &Entity,
    ) -> Result<Vec<(String, String, Option<Entity>)>, ProviderError> {
        self.related_of(entity, &["providesApi", "consumesApi"])
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_ref_parsing() {
        let r: EntityRef = "component:default/payments-api".parse().unwrap();
        assert_eq!(r, EntityRef::new("component", "default", "payments-api"));
        assert_eq!(r.to_string(), "component:default/payments-api");

        // Namespace defaults to `default`.
        let r: EntityRef = "api:payments-v2".parse().unwrap();
        assert_eq!(r.namespace, "default");
        assert_eq!(r.name, "payments-v2");

        // Kind is case-insensitive.
        let r: EntityRef = "Component:default/x".parse().unwrap();
        assert_eq!(r.kind, "component");

        assert!("no-kind-here".parse::<EntityRef>().is_err());
        assert!(":default/x".parse::<EntityRef>().is_err());
    }

    #[test]
    fn entity_deserializes_from_backstage_shape() {
        let raw = serde_json::json!({
            "apiVersion": "backstage.io/v1alpha1",
            "kind": "Component",
            "metadata": {
                "name": "payments-api",
                "namespace": "default",
                "title": "Payments API",
                "description": "Handles payments.",
                "tags": ["payments"],
                "annotations": {"backstage.io/techdocs-ref": "dir:."},
                "links": [{"url": "https://example.com/runbook", "title": "Runbook"}]
            },
            "spec": {"type": "service", "lifecycle": "production", "owner": "payments-team"},
            "relations": [
                {"type": "ownedBy", "targetRef": "group:default/payments-team"},
                {"type": "dependsOn", "targetRef": "component:default/ledger"}
            ]
        });
        let e: Entity = serde_json::from_value(raw).unwrap();
        assert_eq!(e.entity_ref().to_string(), "component:default/payments-api");
        assert_eq!(e.display_title(), "Payments API");
        assert_eq!(
            e.relation_targets("ownedBy"),
            vec!["group:default/payments-team"]
        );
        assert_eq!(e.spec["lifecycle"], "production");
    }
}

//! Context bundle assembly (PRD §7.3).
//!
//! Builds the compact `openade_core::ContextBundle` a session gets injected
//! at launch, by walking the catalog through a [`CatalogProvider`]. Deeper
//! retrieval stays on-demand via the MCP tools.

use openade_core::context::{
    ApiCard, ContextBundle, DocLink, EntityCard, OwnerCard, PriorSessionSummary, RelatedEntity,
};

use crate::provider::{CatalogProvider, Entity, EntityRef, ProviderError};

/// Cap on dependencies/APIs included in the bundle ("N nearest", PRD §7.3);
/// the rest stays reachable through `get_dependencies`.
pub const MAX_RELATED: usize = 8;

fn spec_str(entity: &Entity, key: &str) -> Option<String> {
    entity
        .spec
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn entity_card(entity: &Entity) -> EntityCard {
    EntityCard {
        entity_ref: entity.entity_ref().to_string(),
        title: entity.display_title().to_string(),
        kind: entity.kind.clone(),
        entity_type: spec_str(entity, "type"),
        lifecycle: spec_str(entity, "lifecycle"),
        description: entity.metadata.description.clone(),
        tags: entity.metadata.tags.clone(),
    }
}

/// Build a context bundle for `entity_ref`, attaching any `prior_sessions`
/// the caller has (the daemon queries its transcript index for these).
pub async fn build_context_bundle(
    provider: &dyn CatalogProvider,
    entity_ref: &EntityRef,
    prior_sessions: Vec<PriorSessionSummary>,
) -> Result<ContextBundle, ProviderError> {
    let entity = provider.get_entity(entity_ref).await?;
    let mut bundle = ContextBundle::new(entity_card(&entity));

    if let Some(owner) = provider.owners_of(&entity).await?.into_iter().next() {
        // First catalog link on the owner doubles as a contact channel when
        // present (chat link, team page, ...).
        let contact = owner.metadata.links.first().map(|l| l.url.clone());
        bundle.owner = Some(OwnerCard {
            entity_ref: owner.entity_ref().to_string(),
            display_name: Some(owner.display_title().to_string()),
            contact,
        });
    } else if let Some(raw) = entity.relation_targets("ownedBy").first() {
        // Owner entity unresolvable — keep the bare ref rather than nothing.
        bundle.owner = Some(OwnerCard {
            entity_ref: raw.to_string(),
            display_name: None,
            contact: None,
        });
    }

    for (relation, target_ref, resolved) in provider
        .dependencies_of(&entity)
        .await?
        .into_iter()
        .take(MAX_RELATED)
    {
        bundle.dependencies.push(RelatedEntity {
            entity_ref: target_ref,
            relation,
            title: resolved.as_ref().map(|e| e.display_title().to_string()),
            description: resolved
                .as_ref()
                .and_then(|e| e.metadata.description.clone()),
        });
    }

    for (relation, target_ref, resolved) in provider
        .apis_of(&entity)
        .await?
        .into_iter()
        .take(MAX_RELATED)
    {
        bundle.apis.push(ApiCard {
            entity_ref: target_ref,
            relation,
            api_type: resolved.as_ref().and_then(|e| spec_str(e, "type")),
            description: resolved
                .as_ref()
                .and_then(|e| e.metadata.description.clone()),
        });
    }

    for link in &entity.metadata.links {
        bundle.docs.push(DocLink {
            title: link.title.clone().unwrap_or_else(|| link.url.clone()),
            url: link.url.clone(),
        });
    }

    bundle.prior_sessions = prior_sessions;
    Ok(bundle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::MockProvider;

    #[tokio::test]
    async fn builds_a_complete_bundle() {
        let provider = MockProvider::with_payments_graph();
        let bundle = build_context_bundle(
            &provider,
            &"component:default/payments-api".parse().unwrap(),
            vec![PriorSessionSummary {
                session_id: "s-1".into(),
                harness: Some("claude-code".into()),
                completed_at: None,
                summary: "Added idempotency keys to POST /charges.".into(),
            }],
        )
        .await
        .unwrap();

        assert_eq!(bundle.entity.title, "Payments API");
        assert_eq!(bundle.entity.lifecycle.as_deref(), Some("production"));

        let owner = bundle.owner.as_ref().unwrap();
        assert_eq!(owner.display_name.as_deref(), Some("Payments Team"));
        assert_eq!(
            owner.contact.as_deref(),
            Some("https://chat.example.com/payments-eng")
        );

        assert_eq!(bundle.dependencies.len(), 1);
        assert_eq!(bundle.dependencies[0].title.as_deref(), Some("ledger"));
        assert_eq!(bundle.apis.len(), 1);
        // payments-v2 is not in the mock catalog: kept as a bare ref.
        assert!(bundle.apis[0].api_type.is_none());
        assert_eq!(bundle.docs[0].title, "Runbook");
        assert_eq!(bundle.prior_sessions.len(), 1);

        // The whole point: it fits the injection budget.
        assert!(bundle.within_budget());
        let md = bundle.to_markdown();
        assert!(md.contains("Payments Team"));
        assert!(md.contains("idempotency keys"));
    }

    #[tokio::test]
    async fn entity_without_relations_yields_a_minimal_bundle() {
        let provider = MockProvider::with_payments_graph();
        let bundle = build_context_bundle(
            &provider,
            &"component:default/ledger".parse().unwrap(),
            vec![],
        )
        .await
        .unwrap();
        assert!(bundle.owner.is_none());
        assert!(bundle.dependencies.is_empty());
        assert!(bundle.apis.is_empty());
        assert!(bundle.within_budget());
    }

    #[tokio::test]
    async fn unresolvable_owner_is_kept_as_a_bare_ref() {
        use crate::provider::{CatalogProvider, Entity, EntityRef, ProviderError};
        use async_trait::async_trait;

        /// Delegates to the payments graph but cannot resolve group entities
        /// (e.g. the org's Groups live in a different catalog segment).
        struct NoGroups(MockProvider);

        #[async_trait]
        impl CatalogProvider for NoGroups {
            async fn get_entity(&self, r: &EntityRef) -> Result<Entity, ProviderError> {
                if r.kind == "group" {
                    return Err(ProviderError::NotFound(r.to_string()));
                }
                self.0.get_entity(r).await
            }
            async fn search(&self, q: &str, l: usize) -> Result<Vec<Entity>, ProviderError> {
                self.0.search(q, l).await
            }
            async fn get_techdocs_page(
                &self,
                r: &EntityRef,
                p: &str,
            ) -> Result<String, ProviderError> {
                self.0.get_techdocs_page(r, p).await
            }
        }

        let provider = NoGroups(MockProvider::with_payments_graph());
        let bundle = build_context_bundle(
            &provider,
            &"component:default/payments-api".parse().unwrap(),
            vec![],
        )
        .await
        .unwrap();
        let owner = bundle.owner.clone().unwrap();
        assert_eq!(owner.entity_ref, "group:default/payments-team");
        assert!(owner.display_name.is_none());
        // Markdown falls back to the raw ref when there is no display name.
        assert!(bundle.to_markdown().contains("group:default/payments-team"));
    }

    #[tokio::test]
    async fn missing_entity_propagates_not_found() {
        let provider = MockProvider::with_payments_graph();
        let err = build_context_bundle(
            &provider,
            &"component:default/ghost".parse().unwrap(),
            vec![],
        )
        .await
        .unwrap_err();
        assert!(matches!(err, ProviderError::NotFound(_)));
    }
}

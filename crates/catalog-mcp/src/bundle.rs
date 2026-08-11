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
#[path = "bundle_tests.rs"]
mod tests;

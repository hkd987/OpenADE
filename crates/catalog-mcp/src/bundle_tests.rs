use super::*;
use crate::testutil::MockProvider;

#[tokio::test]
async fn builds_a_complete_bundle() {
    let provider = MockProvider::with_payments_graph();
    let bundle = build_context_bundle(
        &provider,
        &"component:default/payments-api".parse().unwrap(),
        vec![PriorSessionSummary {
            verdict: None,
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
        async fn get_techdocs_page(&self, r: &EntityRef, p: &str) -> Result<String, ProviderError> {
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

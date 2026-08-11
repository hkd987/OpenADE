use super::*;

fn sample_bundle() -> ContextBundle {
    let mut b = ContextBundle::new(EntityCard {
        entity_ref: "component:default/payments-api".into(),
        title: "Payments API".into(),
        kind: "Component".into(),
        entity_type: Some("service".into()),
        lifecycle: Some("production".into()),
        description: Some("Handles payment authorization and capture.".into()),
        tags: vec!["payments".into(), "tier-1".into()],
    });
    b.owner = Some(OwnerCard {
        entity_ref: "group:default/payments-team".into(),
        display_name: Some("Payments Team".into()),
        contact: Some("#payments-eng".into()),
    });
    b.dependencies.push(RelatedEntity {
        entity_ref: "component:default/ledger".into(),
        relation: "dependsOn".into(),
        title: Some("Ledger".into()),
        description: None,
    });
    b.apis.push(ApiCard {
        entity_ref: "api:default/payments-v2".into(),
        relation: "providesApi".into(),
        api_type: Some("openapi".into()),
        description: None,
    });
    b.docs.push(DocLink {
        title: "ADR-007: idempotency keys".into(),
        url: "https://backstage.example.com/docs/default/component/payments-api/adr-007".into(),
    });
    b
}

#[test]
fn round_trips_through_json() {
    let b = sample_bundle();
    let json = serde_json::to_string(&b).unwrap();
    let back: ContextBundle = serde_json::from_str(&json).unwrap();
    assert_eq!(back.version, CONTEXT_BUNDLE_VERSION);
    assert_eq!(back.entity.entity_ref, b.entity.entity_ref);
    assert_eq!(back.dependencies.len(), 1);
}

#[test]
fn markdown_contains_all_sections() {
    let md = sample_bundle().to_markdown();
    assert!(md.contains("# System context: Payments API"));
    assert!(md.contains("`component:default/payments-api`"));
    assert!(md.contains("Payments Team"));
    assert!(md.contains("## Dependencies"));
    assert!(md.contains("## APIs"));
    assert!(md.contains("## Documentation"));
    assert!(md.contains("catalog` MCP tools"));
}

#[test]
fn empty_sections_are_omitted() {
    let b = ContextBundle::new(EntityCard {
        entity_ref: "component:default/tiny".into(),
        title: "Tiny".into(),
        kind: "Component".into(),
        entity_type: None,
        lifecycle: None,
        description: None,
        tags: vec![],
    });
    let md = b.to_markdown();
    assert!(!md.contains("## Dependencies"));
    assert!(!md.contains("## APIs"));
    let json = serde_json::to_value(&b).unwrap();
    assert!(json.get("dependencies").is_none());
    assert!(json.get("owner").is_none());
}

#[test]
fn small_bundle_is_within_budget() {
    let b = sample_bundle();
    assert!(b.estimated_tokens() > 0);
    assert!(b.within_budget());
}

#[test]
fn markdown_falls_back_when_optional_fields_are_missing() {
    let mut b = sample_bundle();
    b.owner = Some(OwnerCard {
        entity_ref: "group:default/mystery-team".into(),
        display_name: None,
        contact: None,
    });
    b.dependencies[0].description = Some("the ledger of record".into());
    b.apis[0].description = Some("v2 of the payments surface".into());
    b.prior_sessions.push(PriorSessionSummary {
        session_id: "s-9".into(),
        harness: None,
        completed_at: None,
        summary: "Fixed the flaky retry test.".into(),
    });

    let md = b.to_markdown();
    // Owner falls back to the bare ref when no display name exists.
    assert!(md.contains("group:default/mystery-team"));
    assert!(md.contains("the ledger of record"));
    assert!(md.contains("v2 of the payments surface"));
    assert!(md.contains("## Prior sessions"));
    assert!(md.contains("Fixed the flaky retry test."));
}

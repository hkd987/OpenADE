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
        verdict: None,
    });

    let md = b.to_markdown();
    // Owner falls back to the bare ref when no display name exists.
    assert!(md.contains("group:default/mystery-team"));
    assert!(md.contains("the ledger of record"));
    assert!(md.contains("v2 of the payments surface"));
    assert!(md.contains("## Prior sessions"));
    assert!(md.contains("Fixed the flaky retry test."));
}

#[test]
fn age_annotations_mark_stale_past_the_boundary() {
    let now = Utc::now();
    let day = chrono::Duration::days(1);
    // 89 / 90 days: fresh; 91: STALE ("informs, never vetoes").
    assert_eq!(age_annotation(now - day * 89, now), "(89 days ago)");
    assert_eq!(age_annotation(now - day * 90, now), "(90 days ago)");
    assert_eq!(age_annotation(now - day * 91, now), "(91 days ago — STALE)");
    // Clock skew (future timestamps) clamps to zero rather than going
    // negative.
    assert_eq!(age_annotation(now + day, now), "(0 days ago)");
}

#[test]
fn prior_sessions_render_verdicts_ages_and_respect_the_cap() {
    let mut b = sample_bundle();
    let now = Utc::now();
    b.prior_sessions.push(PriorSessionSummary {
        session_id: "workspace-7".into(),
        harness: Some("claude-code".into()),
        completed_at: Some(now - chrono::Duration::days(120)),
        summary: "Raised the poll interval".into(),
        verdict: Some("reverted".into()),
    });
    for i in 0..PRIOR_ATTEMPTS_CAP + 2 {
        b.prior_sessions.push(PriorSessionSummary {
            session_id: format!("s-{i}"),
            harness: None,
            completed_at: None,
            summary: format!("attempt {i}"),
            verdict: None,
        });
    }
    let md = b.to_markdown();
    // The verdict and STALE age travel with the entry.
    assert!(md.contains("[verdict: reverted]"), "{md}");
    assert!(md.contains("(120 days ago — STALE)"), "{md}");
    // Only the first PRIOR_ATTEMPTS_CAP entries render.
    assert!(md.contains("attempt 3"), "{md}");
    assert!(!md.contains("attempt 4"), "{md}");
}

#[test]
fn budgeted_rendering_names_what_it_drops() {
    let mut b = sample_bundle();
    // Within budget: identical to the plain rendering, no NOTE.
    assert_eq!(b.to_markdown_budgeted(), b.to_markdown());

    // Blow the budget with bulky dependencies and APIs; docs and prior
    // sessions stay small enough to survive the trim.
    let filler = "x".repeat(400);
    for i in 0..40 {
        b.dependencies.push(RelatedEntity {
            entity_ref: format!("component:default/dep-{i}"),
            relation: "dependsOn".into(),
            title: Some(filler.clone()),
            description: Some(filler.clone()),
        });
        b.apis.push(ApiCard {
            entity_ref: format!("api:default/api-{i}"),
            relation: "providesApi".into(),
            api_type: Some("openapi".into()),
            description: Some(filler.clone()),
        });
    }
    assert!(!b.within_budget());
    let md = b.to_markdown_budgeted();
    // Whole sections were dropped IN ORDER and NAMED — never silently.
    assert!(md.contains("did not fit the context budget"), "{md}");
    assert!(md.contains("Dependencies"), "{md}");
    assert!(!md.contains("component:default/dep-1`"), "{md}");
    // Docs survived (the budget recovered before reaching them).
    assert!(md.contains("## Documentation"), "{md}");

    // Pathological bundle (everything bulky): every section is dropped and
    // every name appears in the note.
    let mut b2 = sample_bundle();
    b2.entity.description = Some("y".repeat(20_000));
    b2.docs.push(DocLink {
        title: "z".repeat(2000),
        url: "https://example.com".into(),
    });
    let md2 = b2.to_markdown_budgeted();
    assert!(
        md2.contains("Dependencies, APIs, Documentation, Prior sessions"),
        "{md2}"
    );
}

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

use super::*;

#[test]
fn fingerprints_are_stable_and_length_prefixed() {
    let a = fingerprint("sentry", &["ref-1", "exception", "NPE in checkout"]);
    let b = fingerprint("sentry", &["ref-1", "exception", "NPE in checkout"]);
    assert_eq!(a, b);
    assert!(a.starts_with("sentry:"), "{a}");
    assert_eq!(a.len(), "sentry:".len() + 32);

    // Length prefixing: concatenation-ambiguous parts must not collide.
    assert_ne!(
        fingerprint("s", &["ab", "c"]),
        fingerprint("s", &["a", "bc"])
    );
    // Source participates too.
    assert_ne!(
        fingerprint("sentry", &["x"]),
        fingerprint("posthog", &["x"])
    );
}

#[test]
fn effective_fingerprint_prefers_the_senders_but_computes_when_absent() {
    let mut sig: SignalIn = serde_json::from_value(serde_json::json!({
        "source": "ci",
        "kind": "regression",
        "severity": "high",
        "title": "flaky checkout test",
    }))
    .unwrap();
    let computed = sig.effective_fingerprint();
    assert_eq!(
        computed,
        fingerprint("ci", &["", "regression", "flaky checkout test"])
    );
    // An explicit fingerprint wins; an empty one does not.
    sig.fingerprint = Some(String::new());
    assert_eq!(sig.effective_fingerprint(), computed);
    sig.fingerprint = Some("ci:custom".into());
    assert_eq!(sig.effective_fingerprint(), "ci:custom");
}

#[test]
fn wire_schema_defaults_and_denies_unknown_fields() {
    // Minimal payload: everything optional defaults.
    let sig: SignalIn = serde_json::from_value(serde_json::json!({
        "source": "webhook",
        "kind": "exception",
        "severity": "critical",
        "title": "boom",
    }))
    .unwrap();
    assert_eq!(sig.body, "");
    assert!(sig.evidence.is_empty());
    assert_eq!(sig.join_keys, JoinKeys::default());
    assert!(sig.join_keys.release.is_none());
    assert_eq!(sig.affected_count, None);
    assert_eq!(sig.raw, serde_json::Value::Null);

    // Senders may not invent fields.
    let err = serde_json::from_value::<SignalIn>(serde_json::json!({
        "source": "webhook",
        "kind": "exception",
        "severity": "critical",
        "title": "boom",
        "made_up": true,
    }))
    .unwrap_err();
    assert!(err.to_string().contains("made_up"), "{err}");
}

#[test]
fn enums_serialize_snake_case_and_severity_orders() {
    assert_eq!(
        serde_json::to_value(SignalKind::UxFriction).unwrap(),
        "ux_friction"
    );
    assert_eq!(SignalKind::UxFriction.as_str(), "ux_friction");
    assert_eq!(SignalKind::Exception.as_str(), "exception");
    assert_eq!(SignalKind::Ticket.as_str(), "ticket");
    assert_eq!(SignalKind::Regression.as_str(), "regression");
    assert_eq!(SignalKind::Custom.as_str(), "custom");
    assert!(Severity::Critical > Severity::High);
    assert!(Severity::Medium > Severity::Low);
    for s in [
        Severity::Low,
        Severity::Medium,
        Severity::High,
        Severity::Critical,
    ] {
        assert_eq!(serde_json::to_value(s).unwrap(), s.as_str());
    }
    for r in [
        DismissReason::IntendedBehavior,
        DismissReason::WontFix,
        DismissReason::Duplicate,
        DismissReason::BadEvidence,
    ] {
        assert_eq!(serde_json::to_value(r).unwrap(), r.as_str());
    }
    for k in [
        OutcomeKind::Merged,
        OutcomeKind::Closed,
        OutcomeKind::Reverted,
        OutcomeKind::Dismissed,
    ] {
        assert_eq!(serde_json::to_value(k).unwrap(), k.as_str());
    }
    // Evidence kinds round-trip.
    let link = EvidenceLink {
        kind: EvidenceKind::StackTrace,
        label: "stack".into(),
        url: "https://sentry.example/e/1".into(),
    };
    let json = serde_json::to_value(&link).unwrap();
    assert_eq!(json["kind"], "stack_trace");
    let back: EvidenceLink = serde_json::from_value(json).unwrap();
    assert_eq!(back, link);
}

#[test]
fn absent_join_keys_are_omitted_not_null() {
    let keys = JoinKeys {
        release: Some("v2.3.0".into()),
        ..JoinKeys::default()
    };
    let json = serde_json::to_value(&keys).unwrap();
    assert_eq!(json, serde_json::json!({ "release": "v2.3.0" }));
}

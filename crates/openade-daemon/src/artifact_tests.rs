use super::*;
use chrono::Utc;
use openade_core::Harness;
use uuid::Uuid;

fn meta() -> SessionMeta {
    let mut m = SessionMeta::new("add retries", Harness::ClaudeCode, "/repo");
    m.entity_ref = Some("component:default/payments-api".into());
    m.branch = Some("openade/add-retries-abc123".into());
    m
}

fn event(kind: EventKind, payload: serde_json::Value) -> SessionEvent {
    SessionEvent {
        session_id: Uuid::new_v4(),
        seq: 1,
        ts: Utc::now(),
        kind,
        payload,
    }
}

const DIFF: &str = "diff --git a/src/client.rs b/src/client.rs\n\
        --- a/src/client.rs\n+++ b/src/client.rs\n@@ -1 +1,2 @@\n retry\n+backoff\n";

#[test]
fn changed_files_come_from_diff_headers() {
    assert_eq!(changed_files(DIFF), vec!["src/client.rs"]);
    assert!(changed_files("").is_empty());
}

#[test]
fn template_summarizer_produces_structured_markdown() {
    let events = vec![
        event(
            EventKind::Prompt,
            serde_json::json!({"text": "add retries"}),
        ),
        event(
            EventKind::Outcome,
            serde_json::json!({
                "summary": "Added exponential backoff to the HTTP client.",
                "decisions": ["cap retries at 5"],
                "gotchas": ["client is shared; backoff must be per-request"],
            }),
        ),
    ];
    let md = TemplateSummarizer.summarize(&meta(), &events, DIFF);
    assert!(md.contains("# Session: add retries"));
    assert!(md.contains("`component:default/payments-api`"));
    assert!(md.contains("> add retries"));
    assert!(md.contains("- `src/client.rs`"));
    assert!(md.contains("exponential backoff"));
    assert!(md.contains("Decisions made"));
    assert!(md.contains("cap retries at 5"));
    assert!(md.contains("Gotchas discovered"));
}

#[test]
fn summary_line_prefers_recorded_outcome() {
    let events = vec![event(
        EventKind::Outcome,
        serde_json::json!({"summary": "Did the thing."}),
    )];
    assert_eq!(
        TemplateSummarizer.summary_line(&meta(), &events, DIFF),
        "Did the thing."
    );
    let line = TemplateSummarizer.summary_line(&meta(), &[], DIFF);
    assert!(line.contains("src/client.rs"), "{line}");
    let line = TemplateSummarizer.summary_line(&meta(), &[], "");
    assert!(line.contains("no file changes"), "{line}");
}

#[test]
fn artifact_slug_is_dated_and_unique_per_session() {
    let m = meta();
    let slug = artifact_slug(&m);
    assert!(slug.contains("add-retries"));
    assert!(slug.contains(&m.created_at.format("%Y-%m-%d").to_string()));
    assert_ne!(artifact_slug(&meta()), slug, "different sessions differ");
}

#[test]
fn summarize_without_prompts_or_changes_stays_structured() {
    let md = TemplateSummarizer.summarize(&meta(), &[], "");
    assert!(!md.contains("## Task"));
    assert!(md.contains("No file changes were recorded"));
    assert!(!md.contains("## Outcome"));
}

#[test]
fn index_upserts_newest_first_and_replaces_republished_entries() {
    let first = IndexEntry {
        file_name: "2026-08-10-add-retries-aa.md".into(),
        title: "add retries".into(),
        summary: "Added retry logic.".into(),
        date: "2026-08-10".into(),
        harness: "claude-code".into(),
        entity: None,
    };
    let index = upsert_index(None, &first);
    assert!(index.starts_with("# Session knowledge index"));
    assert!(index.contains("[add retries](2026-08-10-add-retries-aa.md)"));

    // A second session lands on top.
    let second = IndexEntry {
        file_name: "2026-08-11-fix-flaky-bb.md".into(),
        title: "fix flaky test".into(),
        summary: "Pinned the clock.".into(),
        date: "2026-08-11".into(),
        harness: "gemini-cli".into(),
        entity: Some("repo:acme/payments".into()),
    };
    let index = upsert_index(Some(&index), &second);
    let fix_pos = index.find("fix flaky test").unwrap();
    let retries_pos = index.find("add retries").unwrap();
    assert!(fix_pos < retries_pos, "newest first:\n{index}");

    // Re-publishing the first replaces its line instead of duplicating.
    let index = upsert_index(Some(&index), &first);
    assert_eq!(index.matches("add-retries-aa.md").count(), 1);
    assert_eq!(index.matches("fix-flaky-bb.md").count(), 1);
}

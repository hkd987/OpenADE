use super::*;

#[test]
fn new_session_is_idle_and_serializable() {
    let meta = SessionMeta::new("fix flaky test", Harness::ClaudeCode, "/tmp/repo");
    assert_eq!(meta.state, SessionState::Idle);
    assert!(!meta.state.is_terminal());

    let json = serde_json::to_value(&meta).unwrap();
    assert_eq!(json["state"], "idle");
    assert_eq!(json["harness"], "claude-code");
    // Optional fields are omitted until populated.
    assert!(json.get("worktree_path").is_none());

    let back: SessionMeta = serde_json::from_value(json).unwrap();
    assert_eq!(back.id, meta.id);
}

#[test]
fn terminal_states() {
    assert!(SessionState::Completed.is_terminal());
    assert!(SessionState::Failed.is_terminal());
    assert!(!SessionState::Running.is_terminal());
    assert!(!SessionState::NeedsInput.is_terminal());
}

use super::*;

fn store() -> (tempfile::TempDir, Store) {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path()).unwrap();
    (tmp, store)
}

#[test]
fn tokens_mint_resolve_list_and_revoke() {
    let (_tmp, store) = store();
    let (id, secret) = store.mint_token(DEFAULT_ORG, "casey").unwrap();
    assert!(secret.starts_with("oadk_"));
    assert_eq!(
        store.member_for_token(&secret).unwrap(),
        Some((DEFAULT_ORG, "casey".to_string()))
    );
    assert_eq!(store.tokens(DEFAULT_ORG).unwrap().len(), 1);

    store.revoke_token(DEFAULT_ORG, id).unwrap();
    assert_eq!(store.member_for_token(&secret).unwrap(), None);
    assert!(store.tokens(DEFAULT_ORG).unwrap().is_empty());

    // Revoking a missing token and resolving junk are clean failures.
    assert!(matches!(
        store.revoke_token(DEFAULT_ORG, 999),
        Err(StoreError::NotFound)
    ));
    assert_eq!(store.member_for_token("nope").unwrap(), None);
}

#[test]
fn workspaces_and_sessions_round_trip_org_scoped() {
    let (_tmp, store) = store();
    let ws = store
        .create_workspace(DEFAULT_ORG, "Payments", "Everything payments")
        .unwrap();
    assert_eq!(store.workspaces(DEFAULT_ORG).unwrap().len(), 1);
    assert_eq!(store.workspace(DEFAULT_ORG, ws.id).unwrap().title, "Payments");
    // Wrong org or wrong id → NotFound.
    assert!(matches!(
        store.workspace(2, ws.id),
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.workspace(DEFAULT_ORG, 99),
        Err(StoreError::NotFound)
    ));

    let events = serde_json::json!([{ "kind": "prompt", "payload": { "text": "fix it" } }]);
    let s1 = store
        .upload_session(
            DEFAULT_ORG,
            ws.id,
            "add retries",
            "claude-code",
            Some("repo:acme/payments"),
            Some("openade/add-retries"),
            "Added retries.",
            "# Session\nretries",
            &events,
            "casey",
        )
        .unwrap();
    store
        .upload_session(
            DEFAULT_ORG,
            ws.id,
            "unrelated",
            "codex-cli",
            None,
            None,
            "Other.",
            "# Other",
            &serde_json::json!([]),
            "riley",
        )
        .unwrap();

    // Newest first; entity filter narrows.
    let all = store.sessions(DEFAULT_ORG, ws.id, None).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].title, "unrelated");
    let filtered = store
        .sessions(DEFAULT_ORG, ws.id, Some("repo:acme/payments"))
        .unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].shared_by, "casey");

    // Detail carries markdown + events; missing id → NotFound.
    let detail = store.session(DEFAULT_ORG, s1.id).unwrap();
    assert_eq!(detail.markdown, "# Session\nretries");
    assert_eq!(detail.events[0]["kind"], "prompt");
    assert!(matches!(
        store.session(DEFAULT_ORG, 999),
        Err(StoreError::NotFound)
    ));
    // Upload into a missing workspace fails.
    assert!(store
        .upload_session(
            DEFAULT_ORG,
            99,
            "x",
            "claude-code",
            None,
            None,
            "s",
            "m",
            &serde_json::json!([]),
            "casey"
        )
        .is_err());
}

#[test]
fn corrupt_events_and_broken_db_are_survivable() {
    let (tmp, store) = store();
    let ws = store.create_workspace(DEFAULT_ORG, "W", "").unwrap();
    let s = store
        .upload_session(
            DEFAULT_ORG,
            ws.id,
            "t",
            "claude-code",
            None,
            None,
            "s",
            "m",
            &serde_json::json!([]),
            "casey",
        )
        .unwrap();
    // Corrupt the stored events JSON directly: detail degrades to [].
    {
        let conn = rusqlite::Connection::open(tmp.path().join("server.db")).unwrap();
        conn.execute("UPDATE sessions SET events = 'not json'", [])
            .unwrap();
    }
    let detail = store.session(DEFAULT_ORG, s.id).unwrap();
    assert_eq!(detail.events, serde_json::json!([]));

    // A database that is not a database errors at open-time queries.
    let broken_dir = tmp.path().join("broken");
    std::fs::create_dir_all(&broken_dir).unwrap();
    std::fs::write(broken_dir.join("server.db"), "not a database").unwrap();
    assert!(Store::open(&broken_dir).is_err());
}

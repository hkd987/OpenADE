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
    assert_eq!(
        store.workspace(DEFAULT_ORG, ws.id).unwrap().title,
        "Payments"
    );
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

fn sig(source: &str, title: &str, affected: Option<i64>) -> crate::signal::SignalIn {
    serde_json::from_value(serde_json::json!({
        "source": source,
        "kind": "exception",
        "severity": "high",
        "title": title,
        "body": format!("{title} body"),
        "evidence": [{ "kind": "stack_trace", "label": "trace", "url": "https://s.example/1" }],
        "affected_count": affected,
    }))
    .unwrap()
}

#[test]
fn ingest_dedups_on_fingerprint_and_bumps_recurrences() {
    let (_tmp, store) = store();
    let first = store
        .ingest_signal(DEFAULT_ORG, &sig("sentry", "NPE", Some(10)))
        .unwrap();
    assert!(first.inserted);
    assert!(!first.escalated);

    // Same fingerprint again: not inserted, affected/last_seen bump.
    let again = store
        .ingest_signal(DEFAULT_ORG, &sig("sentry", "NPE", Some(25)))
        .unwrap();
    assert!(!again.inserted);
    assert_eq!(again.item_id, first.item_id);

    let items = store.inbox(DEFAULT_ORG, None).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].affected_count, Some(25));
    assert_eq!(items[0].status, "new");

    // A recurrence with no affected count keeps the last known impact.
    store
        .ingest_signal(DEFAULT_ORG, &sig("sentry", "NPE", None))
        .unwrap();
    let detail = store.inbox_item(DEFAULT_ORG, first.item_id).unwrap();
    assert_eq!(detail.item.affected_count, Some(25));
    assert_eq!(detail.signals.len(), 1);
    assert_eq!(detail.signals[0].evidence[0]["label"], "trace");
    assert!(detail.signals[0].first_seen <= detail.signals[0].last_seen);

    // Different title → different fingerprint → second item.
    store
        .ingest_signal(DEFAULT_ORG, &sig("sentry", "timeout", Some(1)))
        .unwrap();
    assert_eq!(store.inbox(DEFAULT_ORG, None).unwrap().len(), 2);
    // Status filter narrows.
    assert_eq!(store.inbox(DEFAULT_ORG, Some("new")).unwrap().len(), 2);
    assert_eq!(
        store.inbox(DEFAULT_ORG, Some("dismissed")).unwrap().len(),
        0
    );
}

#[test]
fn accept_and_dismiss_stamp_the_actor_and_guard_transitions() {
    let (_tmp, store) = store();
    let a = store
        .ingest_signal(DEFAULT_ORG, &sig("ci", "flaky test", Some(3)))
        .unwrap();
    let item = store.accept_item(DEFAULT_ORG, a.item_id, "casey").unwrap();
    assert_eq!(item.status, "accepted");
    assert_eq!(item.decided_by.as_deref(), Some("casey"));
    assert!(item.decided_at.is_some());

    // Already decided → Conflict; missing → NotFound.
    assert!(matches!(
        store.accept_item(DEFAULT_ORG, a.item_id, "sam"),
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        store.accept_item(DEFAULT_ORG, 9999, "sam"),
        Err(StoreError::NotFound)
    ));

    let b = store
        .ingest_signal(DEFAULT_ORG, &sig("ci", "slow build", Some(4)))
        .unwrap();
    let item = store
        .dismiss_item(
            DEFAULT_ORG,
            b.item_id,
            crate::signal::DismissReason::WontFix,
            "sam",
        )
        .unwrap();
    assert_eq!(item.status, "dismissed");
    assert_eq!(item.dismiss_reason.as_deref(), Some("wont_fix"));
    // Dismissal wrote outcome memory, anchored to the fingerprint.
    let detail = store.inbox_item(DEFAULT_ORG, b.item_id).unwrap();
    assert_eq!(detail.outcomes.len(), 1);
    assert_eq!(detail.outcomes[0].kind, "dismissed");
    assert_eq!(detail.outcomes[0].note.as_deref(), Some("wont_fix"));
}

#[test]
fn escalation_reopens_only_on_3x_growth_with_a_known_snapshot() {
    let (_tmp, store) = store();
    let a = store
        .ingest_signal(DEFAULT_ORG, &sig("posthog", "rage clicks", Some(10)))
        .unwrap();
    store
        .dismiss_item(
            DEFAULT_ORG,
            a.item_id,
            crate::signal::DismissReason::IntendedBehavior,
            "casey",
        )
        .unwrap();

    // 2.9× → stays dismissed.
    let r = store
        .ingest_signal(DEFAULT_ORG, &sig("posthog", "rage clicks", Some(29)))
        .unwrap();
    assert!(!r.escalated);
    assert_eq!(
        store
            .inbox_item(DEFAULT_ORG, a.item_id)
            .unwrap()
            .item
            .status,
        "dismissed"
    );

    // 3× → escalates back to new with an explanatory note.
    let r = store
        .ingest_signal(DEFAULT_ORG, &sig("posthog", "rage clicks", Some(30)))
        .unwrap();
    assert!(r.escalated);
    let item = store.inbox_item(DEFAULT_ORG, a.item_id).unwrap().item;
    assert_eq!(item.status, "new");
    assert!(item.summary.contains("escalated"), "{}", item.summary);
    assert!(item.dismiss_reason.is_none());

    // Unknown impact at dismissal (snapshot NULL/0) never escalates.
    let b = store
        .ingest_signal(DEFAULT_ORG, &sig("posthog", "mystery", None))
        .unwrap();
    store
        .dismiss_item(
            DEFAULT_ORG,
            b.item_id,
            crate::signal::DismissReason::BadEvidence,
            "casey",
        )
        .unwrap();
    let r = store
        .ingest_signal(DEFAULT_ORG, &sig("posthog", "mystery", Some(1000)))
        .unwrap();
    assert!(!r.escalated);
    assert_eq!(
        store
            .inbox_item(DEFAULT_ORG, b.item_id)
            .unwrap()
            .item
            .status,
        "dismissed"
    );

    // Accepted items are never reopened by recurrences.
    let c = store
        .ingest_signal(DEFAULT_ORG, &sig("posthog", "taken", Some(1)))
        .unwrap();
    store.accept_item(DEFAULT_ORG, c.item_id, "casey").unwrap();
    let r = store
        .ingest_signal(DEFAULT_ORG, &sig("posthog", "taken", Some(500)))
        .unwrap();
    assert!(!r.escalated);
    assert_eq!(
        store
            .inbox_item(DEFAULT_ORG, c.item_id)
            .unwrap()
            .item
            .status,
        "accepted"
    );
}

#[test]
fn outcomes_are_idempotent_and_survive_via_the_fingerprint_join() {
    let (_tmp, store) = store();
    let a = store
        .ingest_signal(DEFAULT_ORG, &sig("sentry", "leak", Some(2)))
        .unwrap();
    assert!(store
        .record_outcome(
            DEFAULT_ORG,
            a.item_id,
            "merged",
            Some("https://gh/pr/1"),
            None
        )
        .unwrap());
    // Double-fire is harmless.
    assert!(!store
        .record_outcome(
            DEFAULT_ORG,
            a.item_id,
            "merged",
            Some("https://gh/pr/1"),
            None
        )
        .unwrap());
    // Unknown item is a clean failure.
    assert!(matches!(
        store.record_outcome(DEFAULT_ORG, 999, "merged", None, None),
        Err(StoreError::NotFound)
    ));

    let fp = store
        .inbox_item(DEFAULT_ORG, a.item_id)
        .unwrap()
        .item
        .fingerprint;
    let history = store.outcomes_for_fingerprints(DEFAULT_ORG, &[fp]).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].kind, "merged");
    assert_eq!(history[0].pr_url.as_deref(), Some("https://gh/pr/1"));
    // No fingerprints → no history; unknown fingerprint → empty.
    assert!(store
        .outcomes_for_fingerprints(DEFAULT_ORG, &[])
        .unwrap()
        .is_empty());
    assert!(store
        .outcomes_for_fingerprints(DEFAULT_ORG, &["nope:00".to_string()])
        .unwrap()
        .is_empty());
}

#[test]
fn inbox_rows_are_org_isolated_and_session_verdicts_stick() {
    let (_tmp, store) = store();
    store
        .lock()
        .execute("INSERT INTO orgs (id, name) VALUES (2, 'other')", [])
        .unwrap();
    store
        .ingest_signal(DEFAULT_ORG, &sig("sentry", "ours", Some(1)))
        .unwrap();
    let theirs = store
        .ingest_signal(2, &sig("sentry", "theirs", Some(1)))
        .unwrap();
    assert_eq!(store.inbox(DEFAULT_ORG, None).unwrap().len(), 1);
    assert_eq!(store.inbox(2, None).unwrap().len(), 1);
    // Cross-org item access is NotFound.
    assert!(matches!(
        store.inbox_item(DEFAULT_ORG, theirs.item_id),
        Err(StoreError::NotFound)
    ));

    // Session verdicts: set + read back; unknown session is NotFound.
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
    assert!(s.verdict.is_none());
    store
        .set_session_verdict(DEFAULT_ORG, s.id, "merged")
        .unwrap();
    let listed = store.sessions(DEFAULT_ORG, ws.id, None).unwrap();
    assert_eq!(listed[0].verdict.as_deref(), Some("merged"));
    assert_eq!(
        store
            .session(DEFAULT_ORG, s.id)
            .unwrap()
            .session
            .verdict
            .as_deref(),
        Some("merged")
    );
    assert!(matches!(
        store.set_session_verdict(DEFAULT_ORG, 999, "merged"),
        Err(StoreError::NotFound)
    ));
}

#[test]
fn inbox_survives_reopening_an_existing_database() {
    // The ALTER TABLE migration must tolerate both fresh and re-opened DBs.
    let tmp = tempfile::tempdir().unwrap();
    {
        let store = Store::open(tmp.path()).unwrap();
        store
            .ingest_signal(DEFAULT_ORG, &sig("sentry", "persist", Some(1)))
            .unwrap();
    }
    let store = Store::open(tmp.path()).unwrap();
    assert_eq!(store.inbox(DEFAULT_ORG, None).unwrap().len(), 1);

    // Corrupt evidence JSON degrades to [] instead of failing the read.
    store
        .lock()
        .execute(
            "UPDATE signals SET evidence = 'not json', join_keys = 'nope'",
            [],
        )
        .unwrap();
    let id = store.inbox(DEFAULT_ORG, None).unwrap()[0].id;
    let detail = store.inbox_item(DEFAULT_ORG, id).unwrap();
    assert_eq!(detail.signals[0].evidence, serde_json::json!([]));
    assert_eq!(detail.signals[0].join_keys, serde_json::json!({}));
}

#[test]
fn inbox_db_faults_surface_as_errors_at_every_stage() {
    // Real fault injection: RAISE(ABORT) triggers make individual
    // statements inside the ingest/decide/outcome transactions fail, so
    // every error branch is a branch that has actually run.
    let (_tmp, store) = store();
    let a = store
        .ingest_signal(DEFAULT_ORG, &sig("sentry", "seed", Some(10)))
        .unwrap();

    // item_signals insert fails mid-ingest (fresh fingerprint).
    store
        .lock()
        .execute_batch(
            "CREATE TRIGGER boom_links BEFORE INSERT ON item_signals
             BEGIN SELECT RAISE(ABORT, 'links boom'); END;",
        )
        .unwrap();
    assert!(matches!(
        store.ingest_signal(DEFAULT_ORG, &sig("sentry", "fresh", Some(1))),
        Err(StoreError::Db(_))
    ));
    store
        .lock()
        .execute_batch("DROP TRIGGER boom_links;")
        .unwrap();

    // The escalation reopen UPDATE fails on a dismissed item's 3× return.
    store
        .dismiss_item(
            DEFAULT_ORG,
            a.item_id,
            crate::signal::DismissReason::WontFix,
            "casey",
        )
        .unwrap();
    store
        .lock()
        .execute_batch(
            "CREATE TRIGGER boom_reopen BEFORE UPDATE OF status ON inbox_items
             BEGIN SELECT RAISE(ABORT, 'reopen boom'); END;",
        )
        .unwrap();
    assert!(matches!(
        store.ingest_signal(DEFAULT_ORG, &sig("sentry", "seed", Some(30))),
        Err(StoreError::Db(_))
    ));
    store
        .lock()
        .execute_batch("DROP TRIGGER boom_reopen;")
        .unwrap();

    // decide() fails when the guarded UPDATE itself errors.
    let b = store
        .ingest_signal(DEFAULT_ORG, &sig("sentry", "second", Some(1)))
        .unwrap();
    store
        .lock()
        .execute_batch(
            "CREATE TRIGGER boom_decide BEFORE UPDATE OF decided_by ON inbox_items
             BEGIN SELECT RAISE(ABORT, 'decide boom'); END;",
        )
        .unwrap();
    assert!(matches!(
        store.accept_item(DEFAULT_ORG, b.item_id, "casey"),
        Err(StoreError::Db(_))
    ));
    store
        .lock()
        .execute_batch("DROP TRIGGER boom_decide;")
        .unwrap();

    // record_outcome fails on insert; outcomes_for_fingerprints fails when
    // the outcomes table is gone entirely.
    store
        .lock()
        .execute_batch(
            "CREATE TRIGGER boom_outcome BEFORE INSERT ON outcomes
             BEGIN SELECT RAISE(ABORT, 'outcome boom'); END;",
        )
        .unwrap();
    assert!(matches!(
        store.record_outcome(DEFAULT_ORG, b.item_id, "merged", None, None),
        Err(StoreError::Db(_))
    ));
    store
        .lock()
        .execute_batch("DROP TRIGGER boom_outcome;")
        .unwrap();
    store
        .lock()
        .execute_batch("ALTER TABLE outcomes RENAME TO outcomes_gone;")
        .unwrap();
    assert!(matches!(
        store.outcomes_for_fingerprints(DEFAULT_ORG, &["sentry:00".into()]),
        Err(StoreError::Db(_))
    ));
}

#[test]
fn open_rejects_a_database_where_sessions_is_not_a_table() {
    // The verdict migration ALTERs `sessions`; anything but the benign
    // duplicate-column case must surface, not be swallowed.
    let tmp = tempfile::tempdir().unwrap();
    {
        let conn = rusqlite::Connection::open(tmp.path().join("server.db")).unwrap();
        conn.execute_batch("CREATE VIEW sessions AS SELECT 1 AS id;")
            .unwrap();
    }
    assert!(matches!(Store::open(tmp.path()), Err(StoreError::Db(_))));
}

#[test]
fn poisoned_columns_surface_as_db_errors_not_panics() {
    let (_tmp, store) = store();
    // Signal upsert itself can fail (trigger on signals).
    store
        .lock()
        .execute_batch(
            "CREATE TRIGGER boom_signals BEFORE INSERT ON signals
             BEGIN SELECT RAISE(ABORT, 'signals boom'); END;",
        )
        .unwrap();
    assert!(matches!(
        store.ingest_signal(DEFAULT_ORG, &sig("sentry", "never", Some(1))),
        Err(StoreError::Db(_))
    ));
    store
        .lock()
        .execute_batch("DROP TRIGGER boom_signals;")
        .unwrap();
    // Fresh-item insert can fail (trigger on inbox_items).
    store
        .lock()
        .execute_batch(
            "CREATE TRIGGER boom_items BEFORE INSERT ON inbox_items
             BEGIN SELECT RAISE(ABORT, 'items boom'); END;",
        )
        .unwrap();
    assert!(matches!(
        store.ingest_signal(DEFAULT_ORG, &sig("sentry", "half", Some(1))),
        Err(StoreError::Db(_))
    ));
    store
        .lock()
        .execute_batch("DROP TRIGGER boom_items;")
        .unwrap();

    let a = store
        .ingest_signal(DEFAULT_ORG, &sig("sentry", "ok", Some(10)))
        .unwrap();
    store
        .dismiss_item(
            DEFAULT_ORG,
            a.item_id,
            crate::signal::DismissReason::WontFix,
            "casey",
        )
        .unwrap();

    // SQLite is dynamically typed: a REAL smuggled into affected_count
    // makes the escalation SUM unreadable as an integer.
    store
        .lock()
        .execute("UPDATE signals SET affected_count = 12.5", [])
        .unwrap();
    assert!(matches!(
        store.ingest_signal(DEFAULT_ORG, &sig("sentry", "ok", None)),
        Err(StoreError::Db(_))
    ));
    store
        .lock()
        .execute("UPDATE signals SET affected_count = 10", [])
        .unwrap();

    // A blob smuggled into status (affinity never converts blobs) breaks
    // the decide/record_outcome guards cleanly.
    store
        .lock()
        .execute("UPDATE inbox_items SET status = x'00ff'", [])
        .unwrap();
    assert!(matches!(
        store.accept_item(DEFAULT_ORG, a.item_id, "casey"),
        Err(StoreError::Db(_))
    ));
    assert!(matches!(
        store.record_outcome(DEFAULT_ORG, a.item_id, "merged", None, None),
        Err(StoreError::Db(_))
    ));
    store
        .lock()
        .execute("UPDATE inbox_items SET status = 'new'", [])
        .unwrap();

    // dismiss_item propagates a failure from its outcome write, after the
    // decide succeeded.
    store
        .lock()
        .execute_batch(
            "CREATE TRIGGER boom_dismiss_outcome BEFORE INSERT ON outcomes
             BEGIN SELECT RAISE(ABORT, 'outcome boom'); END;",
        )
        .unwrap();
    assert!(matches!(
        store.dismiss_item(
            DEFAULT_ORG,
            a.item_id,
            crate::signal::DismissReason::Duplicate,
            "sam"
        ),
        Err(StoreError::Db(_))
    ));
}

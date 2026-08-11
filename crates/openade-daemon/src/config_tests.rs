use super::*;

#[test]
fn settings_round_trip_and_tolerate_missing_or_corrupt_files() {
    let tmp = tempfile::tempdir().unwrap();

    // Missing file → defaults (first run).
    assert_eq!(Settings::load(tmp.path()), Settings::default());

    let settings = Settings {
        backstage_base_url: Some("https://backstage.example.com".into()),
        backstage_token: Some("s3cret".into()),
        memory_repo: Some("acme/team-memory".into()),
        onboarded: true,
    };
    settings.save(tmp.path()).unwrap();
    assert_eq!(Settings::load(tmp.path()), settings);

    // Corrupt file → defaults, not a crash.
    std::fs::write(tmp.path().join(CONFIG_FILE), "{not json").unwrap();
    assert_eq!(Settings::load(tmp.path()), Settings::default());

    // Unwritable dir → error surfaced.
    let file_not_dir = tmp.path().join("plain-file");
    std::fs::write(&file_not_dir, "x").unwrap();
    assert!(settings.save(&file_not_dir).is_err());
}

#[test]
fn env_wins_over_stored_settings() {
    let _guard = catalog_mcp::testutil::ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let stored = Settings {
        backstage_base_url: Some("https://stored.example.com".into()),
        backstage_token: Some("stored-token".into()),
        memory_repo: Some("stored/repo".into()),
        onboarded: false,
    };

    // No env: stored settings are effective.
    for var in [
        "BACKSTAGE_BASE_URL",
        "BACKSTAGE_TOKEN",
        "OPENADE_MEMORY_REPO",
    ] {
        std::env::remove_var(var);
    }
    let backstage = stored.effective_backstage().unwrap();
    assert_eq!(backstage.base_url, "https://stored.example.com");
    assert_eq!(backstage.token.as_deref(), Some("stored-token"));
    assert_eq!(
        stored.effective_memory_repo().as_deref(),
        Some("stored/repo")
    );
    assert!(!stored.effective_onboarded());

    // Env set: it wins on every field, and env-configured counts as
    // onboarded (no welcome screen for operators).
    std::env::set_var("BACKSTAGE_BASE_URL", "https://env.example.com");
    std::env::set_var("OPENADE_MEMORY_REPO", "env/repo");
    let backstage = stored.effective_backstage().unwrap();
    assert_eq!(backstage.base_url, "https://env.example.com");
    assert_eq!(stored.effective_memory_repo().as_deref(), Some("env/repo"));
    assert!(stored.effective_onboarded());

    for var in [
        "BACKSTAGE_BASE_URL",
        "BACKSTAGE_TOKEN",
        "OPENADE_MEMORY_REPO",
    ] {
        std::env::remove_var(var);
    }

    // Empty strings are "not configured", and completing onboarding sticks.
    let empty = Settings {
        backstage_base_url: Some(String::new()),
        backstage_token: Some(String::new()),
        memory_repo: Some(String::new()),
        onboarded: true,
    };
    assert!(empty.effective_backstage().is_none());
    assert!(empty.effective_memory_repo().is_none());
    assert!(empty.effective_onboarded());

    // A stored URL with an empty token → no bearer auth.
    let no_token = Settings {
        backstage_base_url: Some("https://stored.example.com".into()),
        backstage_token: None,
        ..Settings::default()
    };
    assert!(no_token.effective_backstage().unwrap().token.is_none());
    assert!(Settings::default().effective_backstage().is_none());
    assert!(Settings::default().effective_memory_repo().is_none());
}

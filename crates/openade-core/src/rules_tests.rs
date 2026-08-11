use super::*;
use tempfile::TempDir;

fn setup(rules: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    init_canonical_rules(dir.path(), rules).unwrap();
    dir
}

#[test]
fn missing_canonical_is_an_error() {
    let dir = TempDir::new().unwrap();
    let err = materialize_rules(dir.path(), &Harness::ALL, false).unwrap_err();
    assert!(matches!(err, RulesError::MissingCanonical(_)));
}

#[test]
fn materializes_all_harness_files() {
    let dir = setup("Always run tests before committing.\n");
    let report = materialize_rules(dir.path(), &Harness::ALL, false).unwrap();
    assert_eq!(report.written.len(), 3);
    for h in Harness::ALL {
        let content = fs::read_to_string(dir.path().join(h.rules_filename())).unwrap();
        assert!(content.contains(GENERATED_MARKER));
        assert!(content.contains("Always run tests"));
    }
}

#[test]
fn is_idempotent() {
    let dir = setup("rule\n");
    materialize_rules(dir.path(), &Harness::ALL, false).unwrap();
    let second = materialize_rules(dir.path(), &Harness::ALL, false).unwrap();
    assert!(second.written.is_empty());
    assert!(second
        .skipped
        .iter()
        .all(|(_, r)| *r == SkipReason::UpToDate));
}

#[test]
fn regenerates_when_canonical_changes() {
    let dir = setup("v1\n");
    materialize_rules(dir.path(), &Harness::ALL, false).unwrap();
    init_canonical_rules(dir.path(), "v2\n").unwrap();
    let report = materialize_rules(dir.path(), &Harness::ALL, false).unwrap();
    assert_eq!(report.written.len(), 3);
    let content = fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
    assert!(content.contains("v2"));
}

#[test]
fn never_clobbers_hand_written_files_without_force() {
    let dir = setup("generated rules\n");
    let hand_written = dir.path().join("CLAUDE.md");
    fs::write(&hand_written, "my precious hand-written rules\n").unwrap();

    let report = materialize_rules(dir.path(), &[Harness::ClaudeCode], false).unwrap();
    assert!(report.written.is_empty());
    assert_eq!(
        report.skipped,
        vec![(hand_written.clone(), SkipReason::HandWritten)]
    );
    let content = fs::read_to_string(&hand_written).unwrap();
    assert_eq!(content, "my precious hand-written rules\n");

    // With force, it is overwritten.
    let report = materialize_rules(dir.path(), &[Harness::ClaudeCode], true).unwrap();
    assert_eq!(report.written.len(), 1);
    assert!(fs::read_to_string(&hand_written)
        .unwrap()
        .contains(GENERATED_MARKER));
}

#[test]
fn io_failures_surface_as_rules_errors() {
    // `.openade` exists as a file: the canonical dir cannot be created.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".openade"), "a file, not a dir").unwrap();
    assert!(matches!(
        init_canonical_rules(dir.path(), "x"),
        Err(RulesError::Io { .. })
    ));

    // `rules.md` exists as a directory: the canonical file cannot be written.
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".openade/rules.md")).unwrap();
    assert!(matches!(
        init_canonical_rules(dir.path(), "x"),
        Err(RulesError::Io { .. })
    ));

    // A target rules file that exists as a directory cannot be read.
    let dir = setup("rule\n");
    fs::create_dir(dir.path().join("CLAUDE.md")).unwrap();
    let err = materialize_rules(dir.path(), &[Harness::ClaudeCode], false).unwrap_err();
    assert!(matches!(err, RulesError::Io { .. }));
    assert!(err.to_string().contains("io error"));
}

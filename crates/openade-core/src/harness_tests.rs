use super::*;

#[test]
fn ids_round_trip_through_serde_and_from_str() {
    for h in Harness::ALL {
        let json = serde_json::to_string(&h).unwrap();
        assert_eq!(json, format!("\"{}\"", h.id()));
        let back: Harness = serde_json::from_str(&json).unwrap();
        assert_eq!(back, h);
        assert_eq!(h.id().parse::<Harness>().unwrap(), h);
    }
}

#[test]
fn short_aliases_parse() {
    assert_eq!("claude".parse::<Harness>().unwrap(), Harness::ClaudeCode);
    assert_eq!("codex".parse::<Harness>().unwrap(), Harness::CodexCli);
    assert_eq!("gemini".parse::<Harness>().unwrap(), Harness::GeminiCli);
    let err = "cursor".parse::<Harness>().unwrap_err();
    assert!(err.to_string().contains("cursor"));
}

#[test]
fn display_and_names() {
    assert_eq!(Harness::ClaudeCode.to_string(), "claude-code");
    assert_eq!(Harness::CodexCli.display_name(), "Codex CLI");
    assert_eq!(Harness::GeminiCli.program(), "gemini");
}

#[test]
fn rules_filenames_are_distinct() {
    let names: std::collections::HashSet<_> =
        Harness::ALL.iter().map(|h| h.rules_filename()).collect();
    assert_eq!(names.len(), Harness::ALL.len());
}

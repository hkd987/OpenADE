use super::*;

fn catalog_server() -> McpServerSpec {
    McpServerSpec {
        name: "catalog".into(),
        transport: McpTransport::Stdio {
            command: "catalog-mcp".into(),
            args: vec!["--stdio".into()],
        },
    }
}

#[test]
fn every_harness_has_an_adapter_with_consistent_identity() {
    for h in Harness::ALL {
        let adapter = adapter_for(h);
        assert_eq!(adapter.harness(), h);
        assert_eq!(adapter.rules_filename(), h.rules_filename());
        assert_eq!(
            adapter.launch_command(&LaunchRequest::default()).program,
            h.program()
        );
    }
}

#[test]
fn launch_command_carries_prompt() {
    let req = LaunchRequest {
        prompt: Some("fix the flaky test".into()),
        mcp_servers: vec![],
    };
    let spec = adapter_for(Harness::ClaudeCode).launch_command(&req);
    assert!(spec.args.contains(&"fix the flaky test".to_string()));
}

#[test]
fn resume_commands_reference_the_session() {
    let spec = adapter_for(Harness::ClaudeCode).resume_command("abc-123");
    assert_eq!(spec.args, vec!["--resume", "abc-123"]);
    let spec = adapter_for(Harness::CodexCli).resume_command("abc-123");
    assert_eq!(spec.args, vec!["resume", "abc-123"]);
    let spec = adapter_for(Harness::GeminiCli).resume_command("abc-123");
    assert!(spec.args.contains(&"abc-123".to_string()));
}

#[test]
fn claude_mcp_registration_is_project_scoped_json() {
    let regs =
        adapter_for(Harness::ClaudeCode).mcp_registrations(Path::new("/wt"), &[catalog_server()]);
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].file, PathBuf::from(".mcp.json"));
    let parsed: serde_json::Value = serde_json::from_str(&regs[0].snippet).unwrap();
    assert_eq!(parsed["mcpServers"]["catalog"]["command"], "catalog-mcp");
}

#[test]
fn codex_mcp_registration_is_user_scoped_toml() {
    let regs =
        adapter_for(Harness::CodexCli).mcp_registrations(Path::new("/wt"), &[catalog_server()]);
    assert_eq!(regs[0].format, "toml");
    // User scope: the daemon must never write this into the worktree.
    assert_eq!(regs[0].scope, RegistrationScope::User);
    assert!(regs[0].snippet.contains("[mcp_servers.catalog]"));
    assert!(regs[0].snippet.contains("command = \"catalog-mcp\""));
}

#[test]
fn project_scopes_match_worktree_relative_files() {
    for h in [Harness::ClaudeCode, Harness::GeminiCli] {
        let regs = adapter_for(h).mcp_registrations(Path::new("/wt"), &[catalog_server()]);
        assert_eq!(regs[0].scope, RegistrationScope::Project);
        assert!(regs[0].file.is_relative());
    }
}

#[test]
fn gemini_mcp_registration_is_project_scoped_settings() {
    let regs =
        adapter_for(Harness::GeminiCli).mcp_registrations(Path::new("/wt"), &[catalog_server()]);
    assert_eq!(regs[0].file, PathBuf::from(".gemini/settings.json"));
    let parsed: serde_json::Value = serde_json::from_str(&regs[0].snippet).unwrap();
    assert!(parsed["mcpServers"]["catalog"].is_object());
}

#[test]
fn codex_http_transport_renders_a_url_entry() {
    let server = McpServerSpec {
        name: "catalog".into(),
        transport: McpTransport::Http {
            url: "http://127.0.0.1:7778/mcp".into(),
        },
    };
    let regs = adapter_for(Harness::CodexCli).mcp_registrations(Path::new("/wt"), &[server]);
    assert!(regs[0]
        .snippet
        .contains("url = \"http://127.0.0.1:7778/mcp\""));
}

#[test]
fn http_transport_is_supported() {
    let server = McpServerSpec {
        name: "catalog".into(),
        transport: McpTransport::Http {
            url: "http://127.0.0.1:7778/mcp".into(),
        },
    };
    let regs = adapter_for(Harness::ClaudeCode).mcp_registrations(Path::new("/wt"), &[server]);
    let parsed: serde_json::Value = serde_json::from_str(&regs[0].snippet).unwrap();
    assert_eq!(
        parsed["mcpServers"]["catalog"]["url"],
        "http://127.0.0.1:7778/mcp"
    );
}

#[test]
fn transcript_hints_live_under_home() {
    let home = Path::new("/home/dev");
    for h in Harness::ALL {
        assert!(adapter_for(h).transcript_hint(home).starts_with(home));
    }
}

#[test]
fn mcp_server_spec_round_trips() {
    let s = catalog_server();
    let json = serde_json::to_value(&s).unwrap();
    assert_eq!(json["transport"], "stdio");
    let back: McpServerSpec = serde_json::from_value(json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn codex_and_gemini_launch_commands_carry_prompts() {
    let req = LaunchRequest {
        prompt: Some("fix it".into()),
        mcp_servers: vec![],
    };
    let spec = adapter_for(Harness::CodexCli).launch_command(&req);
    assert_eq!(spec.args, vec!["fix it"]);
    let spec = adapter_for(Harness::GeminiCli).launch_command(&req);
    assert_eq!(spec.args, vec!["-i", "fix it"]);
}

#[test]
fn opencode_adapter_maps_the_sst_cli_conventions() {
    let req = LaunchRequest {
        prompt: Some("fix it".into()),
        mcp_servers: vec![],
    };
    let spec = adapter_for(Harness::OpenCode).launch_command(&req);
    assert_eq!(spec.program, "opencode");
    assert_eq!(spec.args, vec!["--prompt", "fix it"]);

    let spec = adapter_for(Harness::OpenCode).resume_command("abc-123");
    assert_eq!(spec.args, vec!["--session", "abc-123"]);

    // Project-scoped opencode.json; OpenCode's MCP block uses a command
    // ARRAY for local servers (not command+args) and remote entries carry
    // type + url.
    let regs =
        adapter_for(Harness::OpenCode).mcp_registrations(Path::new("/wt"), &[catalog_server()]);
    assert_eq!(regs[0].scope, RegistrationScope::Project);
    assert_eq!(regs[0].file, PathBuf::from("opencode.json"));
    let parsed: serde_json::Value = serde_json::from_str(&regs[0].snippet).unwrap();
    assert_eq!(parsed["mcp"]["catalog"]["type"], "local");
    assert_eq!(
        parsed["mcp"]["catalog"]["command"],
        serde_json::json!(["catalog-mcp", "--stdio"])
    );

    let remote = McpServerSpec {
        name: "catalog".into(),
        transport: McpTransport::Http {
            url: "http://127.0.0.1:7778/mcp".into(),
        },
    };
    let regs = adapter_for(Harness::OpenCode).mcp_registrations(Path::new("/wt"), &[remote]);
    let parsed: serde_json::Value = serde_json::from_str(&regs[0].snippet).unwrap();
    assert_eq!(parsed["mcp"]["catalog"]["type"], "remote");
    assert_eq!(parsed["mcp"]["catalog"]["url"], "http://127.0.0.1:7778/mcp");

    // Rules come from the shared AGENTS.md convention.
    assert_eq!(adapter_for(Harness::OpenCode).rules_filename(), "AGENTS.md");
}

#[test]
fn copilot_adapter_maps_the_github_cli_conventions() {
    let req = LaunchRequest {
        prompt: Some("fix it".into()),
        mcp_servers: vec![],
    };
    let spec = adapter_for(Harness::CopilotCli).launch_command(&req);
    assert_eq!(spec.program, "copilot");
    assert_eq!(spec.args, vec!["-p", "fix it"]);

    let spec = adapter_for(Harness::CopilotCli).resume_command("abc-123");
    assert_eq!(spec.args, vec!["--resume", "abc-123"]);

    // MCP registration is user-scoped JSON (~/.copilot/mcp-config.json) —
    // surfaced to the user, never written into their home directory.
    let regs =
        adapter_for(Harness::CopilotCli).mcp_registrations(Path::new("/wt"), &[catalog_server()]);
    assert_eq!(regs[0].scope, RegistrationScope::User);
    assert_eq!(regs[0].file, PathBuf::from("~/.copilot/mcp-config.json"));
    assert_eq!(regs[0].format, "json");
    let parsed: serde_json::Value = serde_json::from_str(&regs[0].snippet).unwrap();
    assert_eq!(parsed["mcpServers"]["catalog"]["command"], "catalog-mcp");

    // Rules come from the shared AGENTS.md convention.
    assert_eq!(
        adapter_for(Harness::CopilotCli).rules_filename(),
        "AGENTS.md"
    );
}

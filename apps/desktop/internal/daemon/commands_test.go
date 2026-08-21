package daemon

import (
	"os"
	"path/filepath"
	"testing"
)

func TestDiscoverAgentCommandsUsesProviderInvocationSyntax(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	repo := t.TempDir()
	writeCommandFixture(t, filepath.Join(home, ".codex", "skills", "review-pr", "SKILL.md"), "description: Review a pull request")
	writeCommandFixture(t, filepath.Join(repo, ".agents", "skills", "fix-ci", "SKILL.md"), "description: Fix failing CI")
	commands := discoverAgentCommands(Session{Agent: "codex", RepoRoot: repo})
	if len(commands) != 2 || commands[0].Invocation != "$fix-ci" || commands[1].Invocation != "$review-pr" {
		t.Fatalf("Codex commands = %+v", commands)
	}

	writeCommandFixture(t, filepath.Join(home, ".claude", "commands", "ship.md"), "description: Prepare the branch")
	writeCommandFixture(t, filepath.Join(repo, ".claude", "skills", "audit", "SKILL.md"), "description: Audit the change")
	commands = discoverAgentCommands(Session{Agent: "claude", RepoRoot: repo})
	if len(commands) != 2 || commands[0].Invocation != "/ship" || commands[1].Invocation != "/audit" {
		t.Fatalf("Claude commands = %+v", commands)
	}
}

func writeCommandFixture(t *testing.T, path, contents string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(contents), 0o600); err != nil {
		t.Fatal(err)
	}
}

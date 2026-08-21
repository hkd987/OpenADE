package daemon

import (
	"path/filepath"
	"testing"
	"time"
)

func TestStorePersistsAndInterruptsRunningSessions(t *testing.T) {
	dataDir := t.TempDir()
	store, err := NewStore(dataDir)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now().UTC().Truncate(time.Millisecond)
	session := Session{ID: "session-1", Title: "Fix checkout", Prompt: "Do it", Agent: "claude",
		RepoRoot: "/tmp/repo", WorktreePath: "/tmp/worktree", Branch: "ade/fix-checkout",
		BaseBranch: "main", TicketKey: "ADE-42", Status: "running", PID: 1234,
		CreatedAt: now, UpdatedAt: now}
	if err := store.CreateSession(session); err != nil {
		t.Fatal(err)
	}
	if err := store.Close(); err != nil {
		t.Fatal(err)
	}

	reopened, err := NewStore(dataDir)
	if err != nil {
		t.Fatal(err)
	}
	defer reopened.Close()
	got, err := reopened.GetSession(session.ID)
	if err != nil {
		t.Fatal(err)
	}
	if got.Status != "interrupted" || got.PID != 0 {
		t.Fatalf("restart state = %s pid=%d, want interrupted pid=0", got.Status, got.PID)
	}
	projects, err := reopened.ListProjects()
	if err != nil {
		t.Fatal(err)
	}
	if len(projects) != 1 || projects[0] != session.RepoRoot {
		t.Fatalf("projects = %#v", projects)
	}
	if filepath.Base(dataDir) == "" {
		t.Fatal("expected a real temporary directory")
	}
}

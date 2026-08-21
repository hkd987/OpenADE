package daemon

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func TestWorktreeDiffIncludesCommittedBranchAndWorkingTreeChanges(t *testing.T) {
	repo := createFixtureRepository(t)
	worktree := filepath.Join(t.TempDir(), "worktree")
	if err := createWorktree(context.Background(), repo, worktree, "ade/test-diff", "main"); err != nil {
		t.Fatal(err)
	}
	readme := filepath.Join(worktree, "README.md")
	if err := os.WriteFile(readme, []byte("# fixture\n\ncommitted change\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	runGit := func(args ...string) {
		cmd := exec.Command("git", append([]string{"-C", worktree}, args...)...)
		if output, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v: %s", args, err, output)
		}
	}
	runGit("add", "README.md")
	runGit("commit", "-m", "committed branch change")
	if err := os.WriteFile(filepath.Join(worktree, "working.txt"), []byte("untracked is intentionally omitted\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(readme, []byte("# fixture\n\ncommitted change\nworking change\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	diff, err := worktreeDiff(context.Background(), worktree, "main")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(diff, "committed change") || !strings.Contains(diff, "working change") {
		t.Fatalf("diff does not include the full branch review: %s", diff)
	}
}

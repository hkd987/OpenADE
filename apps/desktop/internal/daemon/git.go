package daemon

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"os/exec"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"time"
)

var branchUnsafe = regexp.MustCompile(`[^a-zA-Z0-9._/-]+`)

func gitOutput(ctx context.Context, repo string, args ...string) (string, error) {
	commandArgs := append([]string{"-C", repo}, args...)
	cmd := exec.CommandContext(ctx, "git", commandArgs...)
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	out, err := cmd.Output()
	if err != nil {
		return "", fmt.Errorf("git %s: %s", strings.Join(args, " "), strings.TrimSpace(stderr.String()))
	}
	return strings.TrimSpace(string(out)), nil
}

func verifyRepository(ctx context.Context, repo string) (string, error) {
	root, err := gitOutput(ctx, repo, "rev-parse", "--show-toplevel")
	if err != nil {
		return "", fmt.Errorf("repository is not a Git worktree: %w", err)
	}
	return filepath.Clean(root), nil
}

func makeBranch(ticket, title, id string) string {
	prefix := "ade"
	if ticket != "" {
		prefix = strings.ToLower(ticket)
	}
	slug := strings.ToLower(strings.TrimSpace(title))
	slug = branchUnsafe.ReplaceAllString(slug, "-")
	slug = strings.Trim(slug, "-./")
	if len(slug) > 42 {
		slug = strings.Trim(slug[:42], "-")
	}
	if slug == "" {
		slug = "session"
	}
	return fmt.Sprintf("%s/%s-%s", prefix, slug, id[:8])
}

func createWorktree(ctx context.Context, repo, path, branch, base string) error {
	if base == "" {
		base = "HEAD"
	}
	cmd := exec.CommandContext(ctx, "git", "-C", repo, "worktree", "add", "-b", branch, path, base)
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("create worktree: %s", strings.TrimSpace(stderr.String()))
	}
	return nil
}

func worktreeDiff(ctx context.Context, path string) (string, error) {
	return gitOutput(ctx, path, "diff", "--no-ext-diff", "--stat", "--patch")
}

func worktreeFiles(ctx context.Context, path string) ([]string, error) {
	out, err := gitOutput(ctx, path, "ls-files", "--cached", "--others", "--exclude-standard")
	if err != nil {
		return nil, err
	}
	if out == "" {
		return []string{}, nil
	}
	files := strings.Split(out, "\n")
	sort.Strings(files)
	return files, nil
}

type PullRequest struct {
	Number         int    `json:"number"`
	Title          string `json:"title"`
	URL            string `json:"url"`
	State          string `json:"state"`
	IsDraft        bool   `json:"isDraft"`
	HeadRefName    string `json:"headRefName"`
	BaseRefName    string `json:"baseRefName"`
	ReviewDecision string `json:"reviewDecision"`
	UpdatedAt      string `json:"updatedAt"`
	Author         struct {
		Login string `json:"login"`
	} `json:"author"`
	Labels []struct {
		Name string `json:"name"`
	} `json:"labels"`
}

func listPullRequests(ctx context.Context, repo string) ([]PullRequest, error) {
	slug, err := githubRepoSlug(ctx, repo)
	if err != nil {
		return nil, err
	}
	cmd := exec.CommandContext(ctx, "gh", "pr", "list", "--repo", slug, "--state", "open", "--limit", "100",
		"--json", "number,title,url,state,isDraft,headRefName,baseRefName,reviewDecision,updatedAt,author,labels")
	out, err := cmd.CombinedOutput()
	if err != nil {
		return nil, fmt.Errorf("gh pr list: %s", strings.TrimSpace(string(out)))
	}
	var prs []PullRequest
	if err := json.Unmarshal(out, &prs); err != nil {
		return nil, fmt.Errorf("decode pull requests: %w", err)
	}
	return prs, nil
}

func createPullRequest(ctx context.Context, session Session, title, body, base string) (string, error) {
	if base == "" {
		base = session.BaseBranch
	}
	if title == "" {
		title = session.Title
	}
	if session.TicketKey != "" && !strings.Contains(title, session.TicketKey) {
		title = session.TicketKey + ": " + title
	}
	if _, err := gitOutput(ctx, session.WorktreePath, "push", "-u", "origin", session.Branch); err != nil {
		return "", err
	}
	slug, err := githubRepoSlug(ctx, session.RepoRoot)
	if err != nil {
		return "", err
	}
	cmd := exec.CommandContext(ctx, "gh", "pr", "create", "--repo", slug, "--draft",
		"--base", base, "--head", session.Branch, "--title", title, "--body", body)
	cmd.Dir = session.WorktreePath
	out, err := cmd.CombinedOutput()
	if err != nil {
		return "", fmt.Errorf("gh pr create: %s", strings.TrimSpace(string(out)))
	}
	return strings.TrimSpace(string(out)), nil
}

func githubRepoSlug(ctx context.Context, repo string) (string, error) {
	if !strings.Contains(repo, string(filepath.Separator)) {
		return strings.TrimSuffix(repo, ".git"), nil
	}
	cmd := exec.CommandContext(ctx, "gh", "repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner")
	cmd.Dir = repo
	out, err := cmd.CombinedOutput()
	if err != nil {
		return "", fmt.Errorf("resolve GitHub repository: %s", strings.TrimSpace(string(out)))
	}
	slug := strings.TrimSpace(string(out))
	if slug == "" {
		return "", fmt.Errorf("repository has no GitHub origin")
	}
	return slug, nil
}

type Ticket struct {
	Key       string `json:"key"`
	Summary   string `json:"summary"`
	Status    string `json:"status"`
	Assignee  string `json:"assignee"`
	URL       string `json:"url"`
	Source    string `json:"source"`
	FetchedAt string `json:"fetched_at"`
}

func fetchJiraTicket(ctx context.Context, key string) (Ticket, error) {
	ticket := Ticket{Key: key, Source: "jira-cli", FetchedAt: time.Now().UTC().Format(time.RFC3339)}
	cmd := exec.CommandContext(ctx, "jira", "issue", "view", key, "--plain")
	out, err := cmd.CombinedOutput()
	if err != nil {
		return ticket, fmt.Errorf("Jira CLI is unavailable or not authenticated: %s", strings.TrimSpace(string(out)))
	}
	lines := strings.Split(strings.TrimSpace(string(out)), "\n")
	if len(lines) > 0 {
		ticket.Summary = strings.TrimSpace(lines[0])
	}
	return ticket, nil
}

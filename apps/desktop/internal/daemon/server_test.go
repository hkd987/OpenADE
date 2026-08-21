package daemon

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestSessionAPIUsesTicketBranchAndStreamsPTY(t *testing.T) {
	repo := createFixtureRepository(t)
	dataDir := t.TempDir()
	d, err := New(Config{DataDir: dataDir, Addr: "127.0.0.1:0"})
	if err != nil {
		t.Fatal(err)
	}
	defer d.store.Close()
	t.Setenv("SHELL", "/bin/sh")

	body, _ := json.Marshal(CreateSessionRequest{Title: "Validate stream", Prompt: "printf 'PTY_OK\\n'",
		Agent: "shell", RepoRoot: repo, BaseBranch: "main", TicketKey: "ADE-101"})
	request := httptest.NewRequest(http.MethodPost, "/api/sessions", bytes.NewReader(body))
	request.Header.Set("Content-Type", "application/json")
	response := httptest.NewRecorder()
	d.routes().ServeHTTP(response, request)
	if response.Code != http.StatusCreated {
		t.Fatalf("create status=%d body=%s", response.Code, response.Body.String())
	}
	var session Session
	if err := json.Unmarshal(response.Body.Bytes(), &session); err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(session.Branch, "ade-101/") {
		t.Fatalf("branch %q does not include ticket", session.Branch)
	}
	deadline := time.Now().Add(4 * time.Second)
	for time.Now().Before(deadline) {
		transcript, err := os.ReadFile(filepath.Join(dataDir, "transcripts", session.ID+".log"))
		if err == nil && strings.Contains(string(transcript), "PTY_OK") {
			break
		}
		time.Sleep(50 * time.Millisecond)
	}
	transcript, err := os.ReadFile(filepath.Join(dataDir, "transcripts", session.ID+".log"))
	if err != nil || !strings.Contains(string(transcript), "PTY_OK") {
		t.Fatalf("transcript missing PTY_OK: %v %q", err, transcript)
	}
	_ = d.sessions.Stop(session.ID)
}

func TestProjectTerminalsAreIndependentAndIndexed(t *testing.T) {
	repo := createFixtureRepository(t)
	d, err := New(Config{DataDir: t.TempDir(), Addr: "127.0.0.1:0"})
	if err != nil {
		t.Fatal(err)
	}
	defer d.store.Close()
	t.Setenv("SHELL", "/bin/sh")

	sessionBody, _ := json.Marshal(CreateSessionRequest{
		Title: "Terminal fixture", Prompt: "printf 'AGENT_ONLY\\n'", Agent: "shell", RepoRoot: repo, BaseBranch: "main",
	})
	createSessionRequest := httptest.NewRequest(http.MethodPost, "/api/sessions", bytes.NewReader(sessionBody))
	createSessionResponse := httptest.NewRecorder()
	d.routes().ServeHTTP(createSessionResponse, createSessionRequest)
	if createSessionResponse.Code != http.StatusCreated {
		t.Fatalf("create session status=%d body=%s", createSessionResponse.Code, createSessionResponse.Body.String())
	}
	var session Session
	if err := json.Unmarshal(createSessionResponse.Body.Bytes(), &session); err != nil {
		t.Fatal(err)
	}

	createTerminalRequest := httptest.NewRequest(http.MethodPost, "/api/sessions/"+session.ID+"/terminals", strings.NewReader(`{"title":"Build"}`))
	createTerminalResponse := httptest.NewRecorder()
	d.routes().ServeHTTP(createTerminalResponse, createTerminalRequest)
	if createTerminalResponse.Code != http.StatusCreated {
		t.Fatalf("create terminal status=%d body=%s", createTerminalResponse.Code, createTerminalResponse.Body.String())
	}
	var terminal TerminalSession
	if err := json.Unmarshal(createTerminalResponse.Body.Bytes(), &terminal); err != nil {
		t.Fatal(err)
	}
	if terminal.SessionID != session.ID || terminal.Cwd != session.WorktreePath || terminal.Title != "Build" {
		t.Fatalf("terminal not scoped to session worktree: %+v", terminal)
	}

	inputBody, _ := json.Marshal(map[string]string{"data": "printf 'TERMINAL_ONLY\\n'\n"})
	inputRequest := httptest.NewRequest(http.MethodPost, "/api/terminals/"+terminal.ID+"/input", bytes.NewReader(inputBody))
	inputResponse := httptest.NewRecorder()
	d.routes().ServeHTTP(inputResponse, inputRequest)
	if inputResponse.Code != http.StatusNoContent {
		t.Fatalf("terminal input status=%d body=%s", inputResponse.Code, inputResponse.Body.String())
	}

	deadline := time.Now().Add(3 * time.Second)
	var transcript []byte
	for time.Now().Before(deadline) {
		transcript, _ = os.ReadFile(filepath.Join(d.config.DataDir, "terminal-transcripts", terminal.ID+".log"))
		if strings.Contains(string(transcript), "TERMINAL_ONLY") {
			break
		}
		time.Sleep(25 * time.Millisecond)
	}
	if !strings.Contains(string(transcript), "TERMINAL_ONLY") || strings.Contains(string(transcript), "AGENT_ONLY") {
		t.Fatalf("terminal stream is not independent: %q", transcript)
	}
	indexed, err := d.store.ListTerminals(session.ID)
	if err != nil || len(indexed) != 1 || indexed[0].ID != terminal.ID {
		t.Fatalf("indexed terminals = %+v, err=%v", indexed, err)
	}
	_ = d.terminals.Stop(terminal.ID)
	_ = d.sessions.Stop(session.ID)
}

func TestAgentTerminalLaunchesCodexTUIAndResumesProviderSession(t *testing.T) {
	repo := createFixtureRepository(t)
	binDir := t.TempDir()
	fakeCodex := filepath.Join(binDir, "codex")
	script := `#!/bin/sh
printf '%s\n' '{"type":"thread.started","thread_id":"thread-tui"}'
printf 'ARGS:%s\n' "$*"
`
	if err := os.WriteFile(fakeCodex, []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", binDir+string(os.PathListSeparator)+os.Getenv("PATH"))
	d, err := New(Config{DataDir: t.TempDir(), Addr: "127.0.0.1:0"})
	if err != nil {
		t.Fatal(err)
	}
	defer d.store.Close()

	session, err := d.sessions.Create(context.Background(), CreateSessionRequest{
		Title: "TUI fixture", Prompt: "first turn", Agent: "codex", RepoRoot: repo, BaseBranch: "main",
	})
	if err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		transcript, _ := os.ReadFile(filepath.Join(d.config.DataDir, "transcripts", session.ID+".log"))
		if strings.Contains(string(transcript), "thread-tui") {
			break
		}
		time.Sleep(20 * time.Millisecond)
	}

	body := `{"kind":"agent","agent":"codex","resume":true}`
	request := httptest.NewRequest(http.MethodPost, "/api/sessions/"+session.ID+"/terminals", strings.NewReader(body))
	response := httptest.NewRecorder()
	d.routes().ServeHTTP(response, request)
	if response.Code != http.StatusCreated {
		t.Fatalf("create agent terminal status=%d body=%s", response.Code, response.Body.String())
	}
	var terminal TerminalSession
	if err := json.Unmarshal(response.Body.Bytes(), &terminal); err != nil {
		t.Fatal(err)
	}
	deadline = time.Now().Add(3 * time.Second)
	var transcript []byte
	for time.Now().Before(deadline) {
		transcript, _ = os.ReadFile(filepath.Join(d.config.DataDir, "terminal-transcripts", terminal.ID+".log"))
		if strings.Contains(string(transcript), "ARGS:") {
			break
		}
		time.Sleep(20 * time.Millisecond)
	}
	output := string(transcript)
	if !strings.Contains(output, "resume --include-non-interactive --no-alt-screen") || !strings.Contains(output, "thread-tui") {
		t.Fatalf("Codex TUI was not resumed directly: %q", output)
	}
}

func TestDirectTUIModeLaunchesAndResumesCodexInTheSessionPTY(t *testing.T) {
	repo := createFixtureRepository(t)
	binDir := t.TempDir()
	fakeCodex := filepath.Join(binDir, "codex")
	script := `#!/bin/sh
printf 'TUI_ARGS:%s\n' "$*"
`
	if err := os.WriteFile(fakeCodex, []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", binDir+string(os.PathListSeparator)+os.Getenv("PATH"))
	d, err := New(Config{DataDir: t.TempDir(), Addr: "127.0.0.1:0"})
	if err != nil {
		t.Fatal(err)
	}
	defer d.store.Close()

	session, err := d.sessions.Create(context.Background(), CreateSessionRequest{
		Title: "Direct TUI", Prompt: "inspect this repo", Agent: "codex", Mode: "tui", RepoRoot: repo, BaseBranch: "main",
	})
	if err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(3 * time.Second)
	transcriptPath := filepath.Join(d.config.DataDir, "transcripts", session.ID+".log")
	var transcript []byte
	for time.Now().Before(deadline) {
		transcript, _ = os.ReadFile(transcriptPath)
		if strings.Contains(string(transcript), "TUI_ARGS:") {
			break
		}
		time.Sleep(20 * time.Millisecond)
	}
	if !strings.Contains(string(transcript), "--no-alt-screen -C "+session.WorktreePath+" inspect this repo") {
		t.Fatalf("direct TUI did not receive the project and prompt: %q", transcript)
	}

	deadline = time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		session, _ = d.store.GetSession(session.ID)
		if session.Status == "completed" {
			break
		}
		time.Sleep(20 * time.Millisecond)
	}
	response := httptest.NewRecorder()
	d.routes().ServeHTTP(response, httptest.NewRequest(http.MethodPost, "/api/sessions/"+session.ID+"/resume-tui", nil))
	if response.Code != http.StatusAccepted {
		t.Fatalf("resume TUI status=%d body=%s", response.Code, response.Body.String())
	}
	deadline = time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		transcript, _ = os.ReadFile(transcriptPath)
		if strings.Contains(string(transcript), "resume --last --no-alt-screen") {
			break
		}
		time.Sleep(20 * time.Millisecond)
	}
	if !strings.Contains(string(transcript), "resume --last --no-alt-screen -C "+session.WorktreePath) {
		t.Fatalf("direct TUI did not resume in the project: %q", transcript)
	}
}

func TestProjectRootScanFindsRepositoriesAndSkipsDependencyTrees(t *testing.T) {
	root := t.TempDir()
	for _, relative := range []string{"team/alpha/.git", "team/beta/.git", "team/node_modules/ignored/.git"} {
		if err := os.MkdirAll(filepath.Join(root, relative), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	d, err := New(Config{DataDir: t.TempDir(), Addr: "127.0.0.1:0"})
	if err != nil {
		t.Fatal(err)
	}
	defer d.store.Close()
	body, _ := json.Marshal(map[string]string{"root": root})
	response := httptest.NewRecorder()
	d.routes().ServeHTTP(response, httptest.NewRequest(http.MethodPost, "/api/projects/scan", bytes.NewReader(body)))
	if response.Code != http.StatusOK {
		t.Fatalf("scan projects status=%d body=%s", response.Code, response.Body.String())
	}
	var payload struct {
		Projects []string `json:"projects"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &payload); err != nil {
		t.Fatal(err)
	}
	want := []string{filepath.Join(root, "team", "alpha"), filepath.Join(root, "team", "beta")}
	if strings.Join(payload.Projects, "|") != strings.Join(want, "|") {
		t.Fatalf("projects=%q want=%q", payload.Projects, want)
	}
}

func TestCompletedCodexSessionCanResumeWithFollowUpMessage(t *testing.T) {
	repo := createFixtureRepository(t)
	binDir := t.TempDir()
	fakeCodex := filepath.Join(binDir, "codex")
	script := `#!/bin/sh
printf '%s\n' '{"type":"thread.started","thread_id":"thread-test"}'
printf '{"type":"item.completed","item":{"type":"agent_message","text":"%s"}}\n' "$*"
`
	if err := os.WriteFile(fakeCodex, []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", binDir+string(os.PathListSeparator)+os.Getenv("PATH"))
	d, err := New(Config{DataDir: t.TempDir(), Addr: "127.0.0.1:0"})
	if err != nil {
		t.Fatal(err)
	}
	defer d.store.Close()

	request := CreateSessionRequest{Title: "Conversation", Prompt: "first turn", Agent: "codex", RepoRoot: repo, BaseBranch: "main"}
	created, err := d.sessions.Create(context.Background(), request)
	if err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		created, _ = d.store.GetSession(created.ID)
		if created.Status == "completed" {
			break
		}
		time.Sleep(20 * time.Millisecond)
	}
	messageBody, _ := json.Marshal(map[string]string{"text": "follow-up turn"})
	messageRequest := httptest.NewRequest(http.MethodPost, "/api/sessions/"+created.ID+"/messages", bytes.NewReader(messageBody))
	messageResponse := httptest.NewRecorder()
	d.routes().ServeHTTP(messageResponse, messageRequest)
	if messageResponse.Code != http.StatusAccepted {
		t.Fatalf("message status=%d body=%s", messageResponse.Code, messageResponse.Body.String())
	}
	replay, _, cancel, err := d.sessions.Subscribe(created.ID)
	if err != nil {
		t.Fatal(err)
	}
	cancel()
	if !strings.Contains(string(replay), "first turn") || !strings.Contains(string(replay), "follow-up turn") {
		t.Fatalf("resumed stream did not replay the full conversation: %s", replay)
	}
	deadline = time.Now().Add(3 * time.Second)
	var transcript []byte
	for time.Now().Before(deadline) {
		transcript, _ = os.ReadFile(filepath.Join(d.config.DataDir, "transcripts", created.ID+".log"))
		if strings.Contains(string(transcript), "resume thread-test follow-up turn") {
			break
		}
		time.Sleep(20 * time.Millisecond)
	}
	if !strings.Contains(string(transcript), "resume thread-test follow-up turn") {
		t.Fatalf("follow-up did not resume provider conversation: %s", transcript)
	}
}

func TestMakeBranchSanitizesInput(t *testing.T) {
	branch := makeBranch("DEV-9", "Fix spaces & checkout!!!", "12345678-abcd")
	if branch != "dev-9/fix-spaces-checkout-12345678" {
		t.Fatalf("branch = %q", branch)
	}
}

func createFixtureRepository(t *testing.T) string {
	t.Helper()
	repo := t.TempDir()
	run := func(args ...string) {
		t.Helper()
		cmd := exec.CommandContext(context.Background(), "git", append([]string{"-C", repo}, args...)...)
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v: %s", args, err, out)
		}
	}
	run("init", "-b", "main")
	run("config", "user.name", "OpenADE Test")
	run("config", "user.email", "openade@example.invalid")
	if err := os.WriteFile(filepath.Join(repo, "README.md"), []byte("# fixture\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	run("add", "README.md")
	run("commit", "-m", "initialize fixture")
	return repo
}

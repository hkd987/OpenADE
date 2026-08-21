package daemon

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
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

func TestProjectRootScanIncludesLocalProviderConversations(t *testing.T) {
	root := t.TempDir()
	repo := filepath.Join(root, "team", "alpha")
	if err := os.MkdirAll(filepath.Join(repo, ".git"), 0o755); err != nil {
		t.Fatal(err)
	}
	home := t.TempDir()
	t.Setenv("HOME", home)
	codexDir := filepath.Join(home, ".codex", "sessions", "2026", "08", "21")
	if err := os.MkdirAll(codexDir, 0o755); err != nil {
		t.Fatal(err)
	}
	history := `{"type":"session_meta","payload":{"id":"existing-codex-session","cwd":` + fmt.Sprintf("%q", repo) + `}}` + "\n" +
		`{"type":"event_msg","payload":{"type":"user_message","message":"Finish the imported parser cleanup"}}` + "\n"
	if err := os.WriteFile(filepath.Join(codexDir, "session.jsonl"), []byte(history), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(home, ".codex"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(home, ".codex", "session_index.jsonl"), []byte(`{"id":"existing-codex-session","thread_name":"Indexed parser cleanup"}`+"\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	claudeDir := filepath.Join(home, ".claude", "projects", "fixture")
	if err := os.MkdirAll(claudeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	claudeHistory := `{"type":"user","sessionId":"existing-claude-session","cwd":` + fmt.Sprintf("%q", repo) + `,"isSidechain":false,"message":{"content":"<scheduled-task name=\"daily-digest\">context</scheduled-task>"}}` + "\n" +
		`{"type":"custom-title","sessionId":"existing-claude-session","customTitle":"Daily project digest"}` + "\n"
	if err := os.WriteFile(filepath.Join(claudeDir, "session.jsonl"), []byte(claudeHistory), 0o600); err != nil {
		t.Fatal(err)
	}
	d, err := New(Config{DataDir: t.TempDir(), Addr: "127.0.0.1:0"})
	if err != nil {
		t.Fatal(err)
	}
	defer d.store.Close()
	body, _ := json.Marshal(map[string]string{"root": root})
	response := httptest.NewRecorder()
	d.routes().ServeHTTP(response, httptest.NewRequest(http.MethodPost, "/api/projects/scan", bytes.NewReader(body)))
	var payload struct {
		Conversations []ExternalConversation `json:"conversations"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &payload); err != nil {
		t.Fatal(err)
	}
	if len(payload.Conversations) != 2 {
		t.Fatalf("conversations=%+v", payload.Conversations)
	}
	byID := map[string]ExternalConversation{}
	for _, conversation := range payload.Conversations {
		byID[conversation.ID] = conversation
	}
	if byID["existing-codex-session"].ProjectRoot != repo || byID["existing-codex-session"].Title != "Indexed parser cleanup" || byID["existing-claude-session"].Title != "Daily project digest" {
		t.Fatalf("conversations=%+v", payload.Conversations)
	}
}

func TestCreateSessionCanResumeAnImportedCodexConversation(t *testing.T) {
	repo := createFixtureRepository(t)
	binDir := t.TempDir()
	fakeCodex := filepath.Join(binDir, "codex")
	if err := os.WriteFile(fakeCodex, []byte("#!/bin/sh\nprintf 'IMPORTED_ARGS:%s\\n' \"$*\"\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", binDir+string(os.PathListSeparator)+os.Getenv("PATH"))
	d, err := New(Config{DataDir: t.TempDir(), Addr: "127.0.0.1:0"})
	if err != nil {
		t.Fatal(err)
	}
	defer d.store.Close()
	session, err := d.sessions.Create(context.Background(), CreateSessionRequest{
		Title: "Imported chat", Agent: "codex", Mode: "tui", ResumeID: "existing-thread", RepoRoot: repo, BaseBranch: "main",
	})
	if err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(3 * time.Second)
	var transcript []byte
	for time.Now().Before(deadline) {
		transcript, _ = os.ReadFile(filepath.Join(d.config.DataDir, "transcripts", session.ID+".log"))
		if strings.Contains(string(transcript), "IMPORTED_ARGS:") {
			break
		}
		time.Sleep(20 * time.Millisecond)
	}
	if !strings.Contains(string(transcript), "resume --include-non-interactive --no-alt-screen") || !strings.Contains(string(transcript), "existing-thread") {
		t.Fatalf("imported conversation was not resumed: %q", transcript)
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

func TestQueuedMessagesPersistAndDrainInSteeredOrder(t *testing.T) {
	repo := createFixtureRepository(t)
	binDir := t.TempDir()
	fakeCodex := filepath.Join(binDir, "codex")
	script := `#!/bin/sh
printf '%s\n' '{"type":"thread.started","thread_id":"thread-queue"}'
case "$*" in *"first turn"*) sleep 0.8 ;; esac
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

	created, err := d.sessions.Create(context.Background(), CreateSessionRequest{Title: "Queued conversation", Prompt: "first turn", Agent: "codex", RepoRoot: repo, BaseBranch: "main"})
	if err != nil {
		t.Fatal(err)
	}
	enqueue := func(text string) QueuedMessage {
		t.Helper()
		body, _ := json.Marshal(map[string]string{"text": text})
		request := httptest.NewRequest(http.MethodPost, "/api/sessions/"+created.ID+"/message-queue", bytes.NewReader(body))
		response := httptest.NewRecorder()
		d.routes().ServeHTTP(response, request)
		if response.Code != http.StatusAccepted {
			t.Fatalf("enqueue status=%d body=%s", response.Code, response.Body.String())
		}
		var message QueuedMessage
		if err := json.Unmarshal(response.Body.Bytes(), &message); err != nil {
			t.Fatal(err)
		}
		return message
	}
	first := enqueue("first queued")
	priority := enqueue("priority queued")
	removed := enqueue("remove me")

	steerRequest := httptest.NewRequest(http.MethodPost, "/api/sessions/"+created.ID+"/message-queue/"+priority.ID+"/steer", nil)
	steerResponse := httptest.NewRecorder()
	d.routes().ServeHTTP(steerResponse, steerRequest)
	if steerResponse.Code != http.StatusNoContent {
		t.Fatalf("steer status=%d body=%s", steerResponse.Code, steerResponse.Body.String())
	}
	deleteRequest := httptest.NewRequest(http.MethodDelete, "/api/sessions/"+created.ID+"/message-queue/"+removed.ID, nil)
	deleteResponse := httptest.NewRecorder()
	d.routes().ServeHTTP(deleteResponse, deleteRequest)
	if deleteResponse.Code != http.StatusNoContent {
		t.Fatalf("delete status=%d body=%s", deleteResponse.Code, deleteResponse.Body.String())
	}

	queued, err := d.store.ListQueuedMessages(created.ID)
	if err != nil {
		t.Fatal(err)
	}
	if len(queued) != 2 || queued[0].ID != priority.ID || queued[1].ID != first.ID {
		t.Fatalf("queued order = %#v", queued)
	}

	deadline := time.Now().Add(5 * time.Second)
	var transcript []byte
	for time.Now().Before(deadline) {
		transcript, _ = os.ReadFile(filepath.Join(d.config.DataDir, "transcripts", created.ID+".log"))
		if strings.Contains(string(transcript), "priority queued") && strings.Contains(string(transcript), "first queued") {
			break
		}
		time.Sleep(25 * time.Millisecond)
	}
	priorityIndex := strings.Index(string(transcript), "priority queued")
	firstIndex := strings.Index(string(transcript), "first queued")
	if priorityIndex < 0 || firstIndex < 0 || priorityIndex >= firstIndex {
		t.Fatalf("queue did not drain in steered order: %s", transcript)
	}
	deadline = time.Now().Add(time.Second)
	for time.Now().Before(deadline) {
		queued, err = d.store.ListQueuedMessages(created.ID)
		if err == nil && len(queued) == 0 {
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	if err != nil || len(queued) != 0 {
		t.Fatalf("queue after drain = %#v err=%v", queued, err)
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

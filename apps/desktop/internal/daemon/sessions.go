package daemon

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"time"

	"github.com/creack/pty"
	"github.com/google/uuid"
)

const maxScrollback = 2 * 1024 * 1024

type CreateSessionRequest struct {
	Title      string `json:"title"`
	Prompt     string `json:"prompt"`
	Agent      string `json:"agent"`
	RepoRoot   string `json:"repo_root"`
	BaseBranch string `json:"base_branch"`
	TicketKey  string `json:"ticket_key"`
	TicketURL  string `json:"ticket_url"`
}

type liveSession struct {
	mu          sync.Mutex
	pty         *os.File
	cmd         *exec.Cmd
	scrollback  []byte
	subscribers map[chan []byte]struct{}
}

type SessionManager struct {
	store   *Store
	dataDir string
	mu      sync.RWMutex
	live    map[string]*liveSession
}

func NewSessionManager(store *Store, dataDir string) *SessionManager {
	return &SessionManager{store: store, dataDir: dataDir, live: make(map[string]*liveSession)}
}

func (m *SessionManager) Create(ctx context.Context, request CreateSessionRequest) (Session, error) {
	request.Title = strings.TrimSpace(request.Title)
	request.RepoRoot = strings.TrimSpace(request.RepoRoot)
	request.Agent = strings.TrimSpace(request.Agent)
	if request.Title == "" || request.RepoRoot == "" {
		return Session{}, fmt.Errorf("title and repository are required")
	}
	if request.Agent == "" {
		request.Agent = "claude"
	}
	if request.BaseBranch == "" {
		request.BaseBranch = "HEAD"
	}
	repo, err := verifyRepository(ctx, request.RepoRoot)
	if err != nil {
		return Session{}, err
	}
	id := uuid.NewString()
	branch := makeBranch(request.TicketKey, request.Title, id)
	repoName := filepath.Base(repo)
	worktree := filepath.Join(m.dataDir, "worktrees", repoName, id)
	if err := os.MkdirAll(filepath.Dir(worktree), 0o755); err != nil {
		return Session{}, err
	}
	if err := createWorktree(ctx, repo, worktree, branch, request.BaseBranch); err != nil {
		return Session{}, err
	}
	now := time.Now().UTC()
	session := Session{ID: id, Title: request.Title, Prompt: request.Prompt, Agent: request.Agent,
		RepoRoot: repo, WorktreePath: worktree, Branch: branch, BaseBranch: request.BaseBranch,
		TicketKey: strings.ToUpper(strings.TrimSpace(request.TicketKey)), TicketURL: request.TicketURL,
		Status: "starting", CreatedAt: now, UpdatedAt: now}
	if err := m.store.CreateSession(session); err != nil {
		return Session{}, err
	}
	if err := m.launch(session); err != nil {
		_ = m.store.UpdateRuntime(id, "failed", 0, nil)
		return m.store.GetSession(id)
	}
	return m.store.GetSession(id)
}

func (m *SessionManager) launch(session Session) error {
	program, args, err := agentCommand(session)
	if err != nil {
		return err
	}
	return m.launchCommand(session, program, args)
}

func (m *SessionManager) launchCommand(session Session, program string, args []string) error {
	cmd := exec.Command(program, args...)
	cmd.Dir = session.WorktreePath
	cmd.Env = append(os.Environ(), "TERM=xterm-256color", "COLORTERM=truecolor", "OPENADE_SESSION_ID="+session.ID)
	ptmx, err := pty.StartWithSize(cmd, &pty.Winsize{Rows: 42, Cols: 120})
	if err != nil {
		return fmt.Errorf("start %s: %w", session.Agent, err)
	}
	live := &liveSession{pty: ptmx, cmd: cmd, subscribers: make(map[chan []byte]struct{})}
	m.mu.Lock()
	m.live[session.ID] = live
	m.mu.Unlock()
	if err := m.store.UpdateRuntime(session.ID, "running", cmd.Process.Pid, nil); err != nil {
		_ = ptmx.Close()
		return err
	}
	transcriptDir := filepath.Join(m.dataDir, "transcripts")
	_ = os.MkdirAll(transcriptDir, 0o755)
	transcript, _ := os.OpenFile(filepath.Join(transcriptDir, session.ID+".log"), os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	go m.readOutput(session.ID, live, transcript)
	go m.wait(session.ID, live, transcript)
	return nil
}

func (m *SessionManager) Resume(session Session, prompt string) error {
	prompt = strings.TrimSpace(prompt)
	if prompt == "" {
		return fmt.Errorf("message is required")
	}
	if _, err := m.getLive(session.ID); err == nil {
		return fmt.Errorf("session is already running")
	}
	transcriptPath := filepath.Join(m.dataDir, "transcripts", session.ID+".log")
	transcript, err := os.ReadFile(transcriptPath)
	if err != nil {
		return fmt.Errorf("read session transcript: %w", err)
	}
	providerID := providerSessionID(transcript, session.Agent)
	if providerID == "" {
		return fmt.Errorf("%s session id was not found in the transcript", session.Agent)
	}
	marker, _ := json.Marshal(map[string]string{"type": "openade.user_message", "text": prompt})
	file, err := os.OpenFile(transcriptPath, os.O_WRONLY|os.O_APPEND, 0o600)
	if err != nil {
		return err
	}
	if _, err := file.Write(append(marker, '\n')); err != nil {
		_ = file.Close()
		return err
	}
	_ = file.Close()

	program, args, err := resumeAgentCommand(session, providerID, prompt)
	if err != nil {
		return err
	}
	return m.launchCommand(session, program, args)
}

func resumeAgentCommand(session Session, providerID, prompt string) (string, []string, error) {
	agent := strings.ToLower(session.Agent)
	name := map[string]string{"claude-code": "claude", "codex-cli": "codex"}[agent]
	if name == "" {
		name = agent
	}
	program, err := resolveProgram(name)
	if err != nil {
		return "", nil, err
	}
	switch name {
	case "claude":
		return program, []string{
			"--resume", providerID, "--print", "--verbose", "--output-format", "stream-json",
			"--include-partial-messages", "--permission-mode", "acceptEdits", prompt,
		}, nil
	case "codex":
		return "/bin/sh", []string{"-lc", `exec "$1" exec --json --sandbox workspace-write resume "$2" "$3" </dev/null`, "openade-codex-resume", program, providerID, prompt}, nil
	default:
		return "", nil, fmt.Errorf("follow-up messages are not supported for %s", session.Agent)
	}
}

func providerSessionID(transcript []byte, agent string) string {
	for _, rawLine := range strings.Split(strings.ReplaceAll(string(transcript), "\r", ""), "\n") {
		line := strings.TrimSpace(rawLine)
		if !strings.HasPrefix(line, "{") {
			continue
		}
		var event map[string]any
		if json.Unmarshal([]byte(line), &event) != nil {
			continue
		}
		if strings.Contains(strings.ToLower(agent), "codex") {
			if id, ok := event["thread_id"].(string); ok && id != "" {
				return id
			}
		} else if id, ok := event["session_id"].(string); ok && id != "" {
			return id
		}
	}
	return ""
}

func agentCommand(session Session) (string, []string, error) {
	agent := strings.ToLower(session.Agent)
	if agent == "shell" {
		if strings.TrimSpace(session.Prompt) != "" {
			return "/bin/sh", []string{"-lc", session.Prompt}, nil
		}
		shell := os.Getenv("SHELL")
		if shell == "" {
			shell = "/bin/zsh"
		}
		return shell, []string{"-l"}, nil
	}
	name := map[string]string{"claude-code": "claude", "codex-cli": "codex", "github-copilot": "copilot"}[agent]
	if name == "" {
		name = agent
	}
	program, err := resolveProgram(name)
	if err != nil {
		return "", nil, err
	}
	switch name {
	case "copilot":
		if session.Prompt != "" {
			return program, []string{"-p", session.Prompt}, nil
		}
	case "opencode":
		if session.Prompt != "" {
			return program, []string{"--prompt", session.Prompt}, nil
		}
	case "claude":
		args := []string{"--name", session.Title}
		if session.Prompt != "" {
			args = append(args,
				"--print", "--verbose", "--output-format", "stream-json",
				"--include-partial-messages", "--permission-mode", "acceptEdits",
				session.Prompt,
			)
		}
		return program, args, nil
	case "codex":
		if session.Prompt != "" {
			// Codex reads stdin in exec mode even with a prompt. Keep stdout on the
			// PTY for live events while closing only stdin so the run can begin.
			return "/bin/sh", []string{"-lc", `exec "$1" exec --json --sandbox workspace-write "$2" </dev/null`, "openade-codex", program, session.Prompt}, nil
		}
	default:
		if session.Prompt != "" {
			return program, []string{session.Prompt}, nil
		}
	}
	return program, nil, nil
}

func resolveProgram(name string) (string, error) {
	if path, err := exec.LookPath(name); err == nil {
		return path, nil
	}
	home, _ := os.UserHomeDir()
	for _, candidate := range []string{filepath.Join(home, ".local", "bin", name), filepath.Join("/opt/homebrew/bin", name), filepath.Join("/usr/local/bin", name)} {
		if info, err := os.Stat(candidate); err == nil && !info.IsDir() {
			return candidate, nil
		}
	}
	return "", fmt.Errorf("%s CLI was not found; install it or add it to PATH", name)
}

func (m *SessionManager) readOutput(id string, live *liveSession, transcript *os.File) {
	reader := bufio.NewReaderSize(live.pty, 32*1024)
	buf := make([]byte, 8192)
	for {
		n, err := reader.Read(buf)
		if n > 0 {
			chunk := append([]byte(nil), buf[:n]...)
			if transcript != nil {
				_, _ = transcript.Write(chunk)
			}
			live.mu.Lock()
			live.scrollback = append(live.scrollback, chunk...)
			if len(live.scrollback) > maxScrollback {
				live.scrollback = append([]byte(nil), live.scrollback[len(live.scrollback)-maxScrollback:]...)
			}
			for subscriber := range live.subscribers {
				select {
				case subscriber <- chunk:
				default:
				}
			}
			live.mu.Unlock()
		}
		if err != nil {
			return
		}
	}
}

func (m *SessionManager) wait(id string, live *liveSession, transcript *os.File) {
	err := live.cmd.Wait()
	code := 0
	status := "completed"
	if err != nil {
		status = "failed"
		if exitErr, ok := err.(*exec.ExitError); ok {
			code = exitErr.ExitCode()
		} else {
			code = 1
		}
	}
	_ = m.store.UpdateRuntime(id, status, 0, &code)
	m.mu.Lock()
	delete(m.live, id)
	m.mu.Unlock()
	if transcript != nil {
		_ = transcript.Close()
	}
	live.mu.Lock()
	for subscriber := range live.subscribers {
		close(subscriber)
	}
	live.subscribers = make(map[chan []byte]struct{})
	live.mu.Unlock()
}

func (m *SessionManager) Write(id, data string) error {
	live, err := m.getLive(id)
	if err != nil {
		return err
	}
	_, err = io.WriteString(live.pty, data)
	return err
}

func (m *SessionManager) Resize(id string, rows, cols uint16) error {
	live, err := m.getLive(id)
	if err != nil {
		return err
	}
	return pty.Setsize(live.pty, &pty.Winsize{Rows: rows, Cols: cols})
}

func (m *SessionManager) Stop(id string) error {
	live, err := m.getLive(id)
	if err != nil {
		return err
	}
	if live.cmd.Process == nil {
		return nil
	}
	return live.cmd.Process.Signal(syscall.SIGTERM)
}

func (m *SessionManager) Subscribe(id string) ([]byte, <-chan []byte, func(), error) {
	live, err := m.getLive(id)
	if err != nil {
		transcript, readErr := os.ReadFile(filepath.Join(m.dataDir, "transcripts", id+".log"))
		if readErr != nil {
			return nil, nil, nil, err
		}
		closed := make(chan []byte)
		close(closed)
		return transcript, closed, func() {}, nil
	}
	ch := make(chan []byte, 128)
	live.mu.Lock()
	initial, readErr := os.ReadFile(filepath.Join(m.dataDir, "transcripts", id+".log"))
	if readErr != nil {
		initial = append([]byte(nil), live.scrollback...)
	}
	live.subscribers[ch] = struct{}{}
	live.mu.Unlock()
	cancel := func() {
		live.mu.Lock()
		if _, ok := live.subscribers[ch]; ok {
			delete(live.subscribers, ch)
			close(ch)
		}
		live.mu.Unlock()
	}
	return initial, ch, cancel, nil
}

func (m *SessionManager) getLive(id string) (*liveSession, error) {
	m.mu.RLock()
	live := m.live[id]
	m.mu.RUnlock()
	if live == nil {
		return nil, fmt.Errorf("session %s is not running", id)
	}
	return live, nil
}

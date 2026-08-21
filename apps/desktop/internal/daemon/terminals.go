package daemon

import (
	"bufio"
	"context"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"sync"
	"syscall"
	"time"

	"github.com/creack/pty"
	"github.com/google/uuid"
)

type TerminalManager struct {
	store    *Store
	dataDir  string
	mu       sync.RWMutex
	live     map[string]*liveSession
	stopping map[string]bool
}

type TerminalLaunch struct {
	Kind   string `json:"kind"`
	Agent  string `json:"agent"`
	Resume bool   `json:"resume"`
}

func NewTerminalManager(store *Store, dataDir string) *TerminalManager {
	return &TerminalManager{
		store: store, dataDir: dataDir,
		live: make(map[string]*liveSession), stopping: make(map[string]bool),
	}
}

func (m *TerminalManager) Create(session Session, title string, launches ...TerminalLaunch) (TerminalSession, error) {
	terminals, err := m.store.ListTerminals(session.ID)
	if err != nil {
		return TerminalSession{}, err
	}
	launch := TerminalLaunch{Kind: "shell"}
	if len(launches) > 0 {
		launch = launches[0]
	}
	if launch.Kind == "" {
		launch.Kind = "shell"
	}
	if title == "" && launch.Kind == "agent" {
		title = fmt.Sprintf("%s TUI", agentDisplayName(launch.Agent, session.Agent))
	}
	if title == "" {
		title = fmt.Sprintf("Terminal %d", len(terminals)+1)
	}
	program, args, err := m.command(session, launch)
	if err != nil {
		return TerminalSession{}, err
	}
	cmd := exec.Command(program, args...)
	cmd.Dir = session.WorktreePath
	cmd.Env = append(os.Environ(), "TERM=xterm-256color", "COLORTERM=truecolor", "OPENADE_SESSION_ID="+session.ID, "OPENADE_TERMINAL_KIND="+launch.Kind)
	ptmx, err := pty.StartWithSize(cmd, &pty.Winsize{Rows: 32, Cols: 100})
	if err != nil {
		return TerminalSession{}, fmt.Errorf("start project terminal: %w", err)
	}
	now := time.Now().UTC()
	terminal := TerminalSession{
		ID: uuid.NewString(), SessionID: session.ID, Title: title, Cwd: session.WorktreePath,
		Status: "running", PID: cmd.Process.Pid, CreatedAt: now, UpdatedAt: now,
	}
	if err := m.store.CreateTerminal(terminal); err != nil {
		terminateUnmanagedProcess(newLiveSession(ptmx, cmd))
		return TerminalSession{}, err
	}
	live := newLiveSession(ptmx, cmd)
	m.mu.Lock()
	m.live[terminal.ID] = live
	m.mu.Unlock()
	transcriptDir := filepath.Join(m.dataDir, "terminal-transcripts")
	_ = os.MkdirAll(transcriptDir, 0o755)
	transcript, _ := os.OpenFile(filepath.Join(transcriptDir, terminal.ID+".log"), os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	go m.readOutput(terminal.ID, live, transcript)
	go m.wait(terminal.ID, live, transcript)
	return terminal, nil
}

func (m *TerminalManager) command(session Session, launch TerminalLaunch) (string, []string, error) {
	if launch.Kind != "agent" {
		shell := os.Getenv("SHELL")
		if shell == "" {
			shell = "/bin/zsh"
		}
		return shell, []string{"-l"}, nil
	}

	agent := launch.Agent
	if agent == "" {
		agent = session.Agent
	}
	agent = map[string]string{"claude-code": "claude", "codex-cli": "codex"}[agent]
	if agent == "" {
		agent = session.Agent
	}
	if agent != "codex" && agent != "claude" {
		return "", nil, fmt.Errorf("direct TUI is only supported for Codex and Claude Code")
	}
	program, err := resolveProgram(agent)
	if err != nil {
		return "", nil, err
	}
	providerID := ""
	if launch.Resume {
		transcript, _ := os.ReadFile(filepath.Join(m.dataDir, "transcripts", session.ID+".log"))
		providerID = providerSessionID(transcript, agent)
	}
	switch agent {
	case "codex":
		if providerID != "" {
			return program, []string{"resume", "--include-non-interactive", "--no-alt-screen", "-C", session.WorktreePath, providerID}, nil
		}
		return program, []string{"--no-alt-screen", "-C", session.WorktreePath}, nil
	case "claude":
		if providerID != "" {
			return program, []string{"--resume", providerID, "--permission-mode", "acceptEdits"}, nil
		}
		return program, []string{"--permission-mode", "acceptEdits"}, nil
	default:
		return "", nil, fmt.Errorf("unsupported TUI agent %s", agent)
	}
}

func agentDisplayName(agent, fallback string) string {
	if agent == "" {
		agent = fallback
	}
	if agent == "codex" || agent == "codex-cli" {
		return "Codex"
	}
	if agent == "claude" || agent == "claude-code" {
		return "Claude"
	}
	return "Agent"
}

func (m *TerminalManager) readOutput(id string, live *liveSession, transcript *os.File) {
	defer close(live.readDone)
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
				live.scrollback = live.scrollback[len(live.scrollback)-maxScrollback:]
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

func (m *TerminalManager) wait(id string, live *liveSession, transcript *os.File) {
	err := live.cmd.Wait()
	_ = signalProcessGroup(live, syscall.SIGTERM)
	_ = live.pty.Close()
	<-live.readDone
	code := 0
	status := "completed"
	m.mu.Lock()
	if m.stopping[id] {
		status = "stopped"
		delete(m.stopping, id)
	} else if err != nil {
		status = "failed"
	}
	delete(m.live, id)
	m.mu.Unlock()
	if exitErr, ok := err.(*exec.ExitError); ok {
		code = exitErr.ExitCode()
	} else if err != nil {
		code = 1
	}
	_ = m.store.UpdateTerminalRuntime(id, status, 0, &code)
	if transcript != nil {
		_ = transcript.Close()
	}
	live.mu.Lock()
	for subscriber := range live.subscribers {
		close(subscriber)
	}
	live.subscribers = make(map[chan []byte]struct{})
	live.mu.Unlock()
	close(live.done)
}

func (m *TerminalManager) Write(id, data string) error {
	live, err := m.getLive(id)
	if err != nil {
		return err
	}
	_, err = io.WriteString(live.pty, data)
	return err
}

func (m *TerminalManager) Resize(id string, rows, cols uint16) error {
	live, err := m.getLive(id)
	if err != nil {
		return err
	}
	return pty.Setsize(live.pty, &pty.Winsize{Rows: rows, Cols: cols})
}

func (m *TerminalManager) Stop(id string) error {
	live, err := m.getLive(id)
	if err != nil {
		terminal, storeErr := m.store.GetTerminal(id)
		if storeErr == nil && terminal.Status != "running" {
			return nil
		}
		return err
	}
	m.mu.Lock()
	m.stopping[id] = true
	m.mu.Unlock()
	if live.cmd.Process == nil {
		return nil
	}
	if err := signalProcessGroup(live, syscall.SIGTERM); err != nil {
		m.mu.Lock()
		delete(m.stopping, id)
		m.mu.Unlock()
		return err
	}
	return nil
}

func (m *TerminalManager) Shutdown(ctx context.Context) {
	m.mu.Lock()
	lives := make([]*liveSession, 0, len(m.live))
	for id, live := range m.live {
		m.stopping[id] = true
		lives = append(lives, live)
	}
	m.mu.Unlock()
	shutdownLiveProcesses(ctx, lives)
}

func (m *TerminalManager) Subscribe(id string) ([]byte, <-chan []byte, func(), error) {
	live, err := m.getLive(id)
	if err != nil {
		if _, storeErr := m.store.GetTerminal(id); storeErr != nil {
			return nil, nil, nil, storeErr
		}
		transcript, _ := os.ReadFile(filepath.Join(m.dataDir, "terminal-transcripts", id+".log"))
		closed := make(chan []byte)
		close(closed)
		return transcript, closed, func() {}, nil
	}
	ch := make(chan []byte, 128)
	live.mu.Lock()
	initial := append([]byte(nil), live.scrollback...)
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

func (m *TerminalManager) getLive(id string) (*liveSession, error) {
	m.mu.RLock()
	live := m.live[id]
	m.mu.RUnlock()
	if live == nil {
		return nil, fmt.Errorf("terminal %s is not running", id)
	}
	return live, nil
}

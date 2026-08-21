package daemon

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"syscall"
	"testing"
	"time"
)

func TestSessionLifecycleClosesPTYAndSubscribers(t *testing.T) {
	d, err := New(Config{DataDir: t.TempDir(), Addr: "127.0.0.1:0"})
	if err != nil {
		t.Fatal(err)
	}
	defer d.store.Close()

	now := time.Now().UTC()
	session := Session{
		ID: "session-lifecycle", Title: "Lifecycle", Agent: "shell", Mode: "chat",
		RepoRoot: t.TempDir(), WorktreePath: t.TempDir(), Branch: "lifecycle", BaseBranch: "main",
		Status: "starting", CreatedAt: now, UpdatedAt: now,
	}
	if err := d.store.CreateSession(session); err != nil {
		t.Fatal(err)
	}
	if err := d.sessions.launchCommand(session, "/bin/sh", []string{"-c", "sleep 0.1; printf 'SESSION_DONE\\n'"}); err != nil {
		t.Fatal(err)
	}
	live, err := d.sessions.getLive(session.ID)
	if err != nil {
		t.Fatal(err)
	}
	_, _, cancel, err := d.sessions.Subscribe(session.ID)
	if err != nil {
		t.Fatal(err)
	}
	cancel()
	assertNoSubscribers(t, live)
	waitUntilNotLive(t, func() error { _, err := d.sessions.getLive(session.ID); return err })
	assertClosedFile(t, live.pty)
	transcript, err := os.ReadFile(filepath.Join(d.config.DataDir, "transcripts", session.ID+".log"))
	if err != nil || len(transcript) == 0 {
		t.Fatalf("session transcript was not flushed: %q, err=%v", transcript, err)
	}
}

func TestTerminalLifecycleClosesPTYAndSubscribers(t *testing.T) {
	d, err := New(Config{DataDir: t.TempDir(), Addr: "127.0.0.1:0"})
	if err != nil {
		t.Fatal(err)
	}
	defer d.store.Close()
	t.Setenv("SHELL", "/bin/sh")

	now := time.Now().UTC()
	session := Session{
		ID: "terminal-parent", Title: "Terminal parent", Agent: "shell", Mode: "chat",
		RepoRoot: t.TempDir(), WorktreePath: t.TempDir(), Branch: "terminal-parent", BaseBranch: "main",
		Status: "completed", CreatedAt: now, UpdatedAt: now,
	}
	if err := d.store.CreateSession(session); err != nil {
		t.Fatal(err)
	}
	terminal, err := d.terminals.Create(session, "Lifecycle terminal")
	if err != nil {
		t.Fatal(err)
	}
	live, err := d.terminals.getLive(terminal.ID)
	if err != nil {
		t.Fatal(err)
	}
	_, _, cancel, err := d.terminals.Subscribe(terminal.ID)
	if err != nil {
		t.Fatal(err)
	}
	cancel()
	assertNoSubscribers(t, live)
	if err := d.terminals.Write(terminal.ID, "printf 'TERMINAL_DONE\\n'; exit\n"); err != nil {
		t.Fatal(err)
	}
	waitUntilNotLive(t, func() error { _, err := d.terminals.getLive(terminal.ID); return err })
	assertClosedFile(t, live.pty)
}

func TestDaemonShutdownStopsManagedProcesses(t *testing.T) {
	d, err := New(Config{DataDir: t.TempDir(), Addr: "127.0.0.1:0"})
	if err != nil {
		t.Fatal(err)
	}

	now := time.Now().UTC()
	session := Session{
		ID: "shutdown-session", Title: "Shutdown", Agent: "shell", Mode: "chat",
		RepoRoot: t.TempDir(), WorktreePath: t.TempDir(), Branch: "shutdown", BaseBranch: "main",
		Status: "starting", CreatedAt: now, UpdatedAt: now,
	}
	if err := d.store.CreateSession(session); err != nil {
		t.Fatal(err)
	}
	if err := d.sessions.launchCommand(session, "/bin/sh", []string{"-c", "exec sleep 30"}); err != nil {
		t.Fatal(err)
	}
	live, err := d.sessions.getLive(session.ID)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if live.cmd.Process != nil {
			_ = live.cmd.Process.Kill()
		}
	})

	ctx, cancel := context.WithCancel(context.Background())
	runDone := make(chan error, 1)
	go func() { runDone <- d.Run(ctx) }()
	time.Sleep(25 * time.Millisecond)
	cancel()
	select {
	case err := <-runDone:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("daemon shutdown timed out")
	}
	if err := live.cmd.Process.Signal(syscall.Signal(0)); err == nil {
		t.Fatal("managed session process is still alive after daemon shutdown")
	}
	assertClosedFile(t, live.pty)
}

func waitUntilNotLive(t *testing.T, lookup func() error) {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		if lookup() != nil {
			return
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatal("managed process remained indexed after exit")
}

func assertNoSubscribers(t *testing.T, live *liveSession) {
	t.Helper()
	live.mu.Lock()
	defer live.mu.Unlock()
	if len(live.subscribers) != 0 {
		t.Fatalf("subscriber count = %d, want 0", len(live.subscribers))
	}
}

func assertClosedFile(t *testing.T, file *os.File) {
	t.Helper()
	if _, err := file.Stat(); !errors.Is(err, os.ErrClosed) {
		t.Fatalf("PTY file is still open: %v", err)
	}
}

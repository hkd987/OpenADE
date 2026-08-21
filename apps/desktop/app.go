package main

import (
	"context"
	"fmt"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"sync"
	"time"

	"github.com/hkd987/OpenADE/apps/desktop/internal/daemon"
	"github.com/wailsapp/wails/v2/pkg/runtime"
)

const daemonURL = "http://127.0.0.1:7433"

type App struct {
	ctx       context.Context
	mu        sync.RWMutex
	daemonErr string
}

func NewApp() *App { return &App{} }

func (a *App) startup(ctx context.Context) {
	a.ctx = ctx
	go func() {
		err := ensureDaemon()
		a.mu.Lock()
		if err != nil {
			a.daemonErr = err.Error()
		} else {
			a.daemonErr = ""
		}
		a.mu.Unlock()
		runtime.EventsEmit(ctx, "daemon:ready", err == nil)
	}()
}

// shutdown intentionally leaves the daemon alive so PTYs continue running.
func (a *App) shutdown(context.Context) {}

func (a *App) DaemonURL() string { return daemonURL }

func (a *App) DaemonStatus() map[string]any {
	a.mu.RLock()
	defer a.mu.RUnlock()
	return map[string]any{"url": daemonURL, "ready": a.daemonErr == "", "error": a.daemonErr}
}

func (a *App) OpenExternal(url string) {
	if a.ctx != nil {
		runtime.BrowserOpenURL(a.ctx, url)
	}
}

func ensureDaemon() error {
	client := &http.Client{Timeout: 350 * time.Millisecond}
	if healthy(client) {
		return nil
	}
	executable, err := os.Executable()
	if err != nil {
		return fmt.Errorf("locate OpenADE executable: %w", err)
	}
	config := daemon.DefaultConfig()
	if err := os.MkdirAll(config.DataDir, 0o700); err != nil {
		return fmt.Errorf("create data directory: %w", err)
	}
	logFile, err := os.OpenFile(filepath.Join(config.DataDir, "daemon.log"), os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o600)
	if err != nil {
		return fmt.Errorf("open daemon log: %w", err)
	}
	command := exec.Command(executable, "--daemon", "--data-dir", config.DataDir)
	command.Stdout = logFile
	command.Stderr = logFile
	command.Stdin = nil
	command.Env = append(os.Environ(), "OPENADE_DATA_DIR="+config.DataDir)
	if err := command.Start(); err != nil {
		logFile.Close()
		return fmt.Errorf("start daemon: %w", err)
	}
	_ = command.Process.Release()
	_ = logFile.Close()
	deadline := time.Now().Add(8 * time.Second)
	for time.Now().Before(deadline) {
		if healthy(client) {
			return nil
		}
		time.Sleep(120 * time.Millisecond)
	}
	return fmt.Errorf("daemon did not become ready; see %s", filepath.Join(config.DataDir, "daemon.log"))
}

func healthy(client *http.Client) bool {
	response, err := client.Get(daemonURL + "/api/health")
	if err != nil {
		return false
	}
	defer response.Body.Close()
	return response.StatusCode == http.StatusOK
}

package daemon

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/gorilla/websocket"
)

type Config struct {
	Addr    string
	DataDir string
}

type Daemon struct {
	config   Config
	store    *Store
	sessions *SessionManager
	server   *http.Server
}

func DefaultConfig() Config {
	dataDir := os.Getenv("OPENADE_DATA_DIR")
	if dataDir == "" {
		configDir, _ := os.UserConfigDir()
		dataDir = filepath.Join(configDir, "OpenADE")
	}
	addr := os.Getenv("OPENADE_DAEMON_ADDR")
	if addr == "" {
		addr = "127.0.0.1:7433"
	}
	return Config{Addr: addr, DataDir: dataDir}
}

func New(config Config) (*Daemon, error) {
	if config.Addr == "" {
		config.Addr = "127.0.0.1:7433"
	}
	if config.DataDir == "" {
		config.DataDir = DefaultConfig().DataDir
	}
	if err := os.MkdirAll(config.DataDir, 0o700); err != nil {
		return nil, err
	}
	store, err := NewStore(config.DataDir)
	if err != nil {
		return nil, err
	}
	d := &Daemon{config: config, store: store, sessions: NewSessionManager(store, config.DataDir)}
	d.server = &http.Server{Addr: config.Addr, Handler: d.routes(), ReadHeaderTimeout: 5 * time.Second}
	return d, nil
}

func (d *Daemon) Run(ctx context.Context) error {
	listener, err := net.Listen("tcp", d.config.Addr)
	if err != nil {
		return fmt.Errorf("listen on %s: %w", d.config.Addr, err)
	}
	errCh := make(chan error, 1)
	go func() { errCh <- d.server.Serve(listener) }()
	select {
	case <-ctx.Done():
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = d.server.Shutdown(shutdownCtx)
		_ = d.store.Close()
		return nil
	case err := <-errCh:
		_ = d.store.Close()
		if errors.Is(err, http.ErrServerClosed) {
			return nil
		}
		return err
	}
}

func (d *Daemon) routes() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /api/health", func(w http.ResponseWriter, _ *http.Request) {
		writeJSON(w, http.StatusOK, map[string]any{"ok": true, "pid": os.Getpid(), "version": "0.3.0-go"})
	})
	mux.HandleFunc("GET /api/meta", d.handleMeta)
	mux.HandleFunc("GET /api/sessions", d.handleListSessions)
	mux.HandleFunc("POST /api/sessions", d.handleCreateSession)
	mux.HandleFunc("GET /api/projects", d.handleProjects)
	mux.HandleFunc("GET /api/sessions/{id}", d.handleGetSession)
	mux.HandleFunc("GET /api/sessions/{id}/stream", d.handleStream)
	mux.HandleFunc("POST /api/sessions/{id}/input", d.handleInput)
	mux.HandleFunc("POST /api/sessions/{id}/resize", d.handleResize)
	mux.HandleFunc("POST /api/sessions/{id}/stop", d.handleStop)
	mux.HandleFunc("GET /api/sessions/{id}/diff", d.handleDiff)
	mux.HandleFunc("GET /api/sessions/{id}/files", d.handleFiles)
	mux.HandleFunc("GET /api/github/pull-requests", d.handlePullRequests)
	mux.HandleFunc("POST /api/github/pull-requests", d.handleCreatePullRequest)
	mux.HandleFunc("GET /api/jira/tickets/{key}", d.handleJiraTicket)
	return cors(mux)
}

func (d *Daemon) handleProjects(w http.ResponseWriter, _ *http.Request) {
	projects, err := d.store.ListProjects()
	if err != nil {
		writeError(w, http.StatusInternalServerError, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"projects": projects})
}

func cors(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		origin := r.Header.Get("Origin")
		if origin == "" || strings.HasPrefix(origin, "http://localhost:") || strings.HasPrefix(origin, "http://127.0.0.1:") || strings.HasPrefix(origin, "wails://") {
			w.Header().Set("Access-Control-Allow-Origin", origin)
		}
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type")
		w.Header().Set("Access-Control-Allow-Methods", "GET,POST,OPTIONS")
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		next.ServeHTTP(w, r)
	})
}

func (d *Daemon) handleMeta(w http.ResponseWriter, r *http.Request) {
	agents := []map[string]any{}
	for _, name := range []string{"claude", "codex", "copilot", "opencode"} {
		path, err := resolveProgram(name)
		agents = append(agents, map[string]any{"id": name, "available": err == nil, "path": path})
	}
	_, ghErr := resolveProgram("gh")
	writeJSON(w, http.StatusOK, map[string]any{"agents": agents, "github_available": ghErr == nil, "data_dir": d.config.DataDir})
}

func (d *Daemon) handleListSessions(w http.ResponseWriter, _ *http.Request) {
	sessions, err := d.store.ListSessions()
	if err != nil {
		writeError(w, http.StatusInternalServerError, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"sessions": sessions})
}

func (d *Daemon) handleCreateSession(w http.ResponseWriter, r *http.Request) {
	var request CreateSessionRequest
	if err := json.NewDecoder(r.Body).Decode(&request); err != nil {
		writeError(w, http.StatusBadRequest, err)
		return
	}
	session, err := d.sessions.Create(r.Context(), request)
	if err != nil {
		writeError(w, http.StatusBadRequest, err)
		return
	}
	writeJSON(w, http.StatusCreated, session)
}

func (d *Daemon) handleGetSession(w http.ResponseWriter, r *http.Request) {
	session, err := d.store.GetSession(r.PathValue("id"))
	if err != nil {
		writeStoreError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, session)
}

var upgrader = websocket.Upgrader{ReadBufferSize: 4096, WriteBufferSize: 4096, CheckOrigin: func(r *http.Request) bool {
	origin := r.Header.Get("Origin")
	return origin == "" || strings.Contains(origin, "localhost") || strings.Contains(origin, "127.0.0.1") || strings.HasPrefix(origin, "wails://")
}}

func (d *Daemon) handleStream(w http.ResponseWriter, r *http.Request) {
	initial, output, cancel, err := d.sessions.Subscribe(r.PathValue("id"))
	if err != nil {
		writeError(w, http.StatusNotFound, err)
		return
	}
	conn, err := upgrader.Upgrade(w, r, nil)
	if err != nil {
		cancel()
		return
	}
	defer conn.Close()
	defer cancel()
	if len(initial) > 0 {
		_ = conn.WriteJSON(map[string]any{"type": "output", "data": string(initial), "replay": true})
	}
	for chunk := range output {
		if err := conn.WriteJSON(map[string]any{"type": "output", "data": string(chunk)}); err != nil {
			return
		}
	}
	session, _ := d.store.GetSession(r.PathValue("id"))
	_ = conn.WriteJSON(map[string]any{"type": "status", "status": session.Status, "exit_code": session.ExitCode})
}

func (d *Daemon) handleInput(w http.ResponseWriter, r *http.Request) {
	var body struct {
		Data string `json:"data"`
	}
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		writeError(w, 400, err)
		return
	}
	if err := d.sessions.Write(r.PathValue("id"), body.Data); err != nil {
		writeError(w, 409, err)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

func (d *Daemon) handleResize(w http.ResponseWriter, r *http.Request) {
	var body struct{ Rows, Cols uint16 }
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		writeError(w, 400, err)
		return
	}
	if err := d.sessions.Resize(r.PathValue("id"), body.Rows, body.Cols); err != nil {
		writeError(w, 409, err)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

func (d *Daemon) handleStop(w http.ResponseWriter, r *http.Request) {
	if err := d.sessions.Stop(r.PathValue("id")); err != nil {
		writeError(w, 409, err)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

func (d *Daemon) handleDiff(w http.ResponseWriter, r *http.Request) {
	session, err := d.store.GetSession(r.PathValue("id"))
	if err != nil {
		writeStoreError(w, err)
		return
	}
	diff, err := worktreeDiff(r.Context(), session.WorktreePath)
	if err != nil {
		writeError(w, 500, err)
		return
	}
	writeJSON(w, 200, map[string]any{"diff": diff})
}

func (d *Daemon) handleFiles(w http.ResponseWriter, r *http.Request) {
	session, err := d.store.GetSession(r.PathValue("id"))
	if err != nil {
		writeStoreError(w, err)
		return
	}
	files, err := worktreeFiles(r.Context(), session.WorktreePath)
	if err != nil {
		writeError(w, 500, err)
		return
	}
	writeJSON(w, 200, map[string]any{"files": files})
}

func (d *Daemon) handlePullRequests(w http.ResponseWriter, r *http.Request) {
	repo := r.URL.Query().Get("repo")
	if repo == "" {
		writeError(w, 400, fmt.Errorf("repo is required"))
		return
	}
	prs, err := listPullRequests(r.Context(), repo)
	if err != nil {
		writeError(w, 502, err)
		return
	}
	writeJSON(w, 200, map[string]any{"pull_requests": prs})
}

func (d *Daemon) handleCreatePullRequest(w http.ResponseWriter, r *http.Request) {
	var body struct{ SessionID, Title, Body, Base string }
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		writeError(w, 400, err)
		return
	}
	session, err := d.store.GetSession(body.SessionID)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	url, err := createPullRequest(r.Context(), session, body.Title, body.Body, body.Base)
	if err != nil {
		writeError(w, 502, err)
		return
	}
	_ = d.store.SetPR(session.ID, url)
	writeJSON(w, http.StatusCreated, map[string]any{"url": url})
}

func (d *Daemon) handleJiraTicket(w http.ResponseWriter, r *http.Request) {
	key := strings.ToUpper(strings.TrimSpace(r.PathValue("key")))
	if key == "" {
		writeError(w, 400, fmt.Errorf("ticket key is required"))
		return
	}
	ticket, err := fetchJiraTicket(r.Context(), key)
	if err != nil {
		writeError(w, 502, err)
		return
	}
	writeJSON(w, 200, ticket)
}

func writeStoreError(w http.ResponseWriter, err error) {
	if IsNotFound(err) {
		writeError(w, 404, fmt.Errorf("session not found"))
		return
	}
	writeError(w, 500, err)
}

func writeError(w http.ResponseWriter, status int, err error) {
	writeJSON(w, status, map[string]string{"error": err.Error()})
}
func writeJSON(w http.ResponseWriter, status int, value any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(value)
}

func parseUint16(value string, fallback uint16) uint16 {
	n, err := strconv.ParseUint(value, 10, 16)
	if err != nil {
		return fallback
	}
	return uint16(n)
}

package daemon

import (
	"database/sql"
	"errors"
	"fmt"
	"path/filepath"
	"strings"
	"time"

	_ "github.com/mattn/go-sqlite3"
)

type Session struct {
	ID           string     `json:"id"`
	Title        string     `json:"title"`
	Prompt       string     `json:"prompt"`
	Agent        string     `json:"agent"`
	Mode         string     `json:"mode"`
	RepoRoot     string     `json:"repo_root"`
	WorktreePath string     `json:"worktree_path"`
	Branch       string     `json:"branch"`
	BaseBranch   string     `json:"base_branch"`
	TicketKey    string     `json:"ticket_key,omitempty"`
	TicketURL    string     `json:"ticket_url,omitempty"`
	Status       string     `json:"status"`
	PID          int        `json:"pid,omitempty"`
	ExitCode     *int       `json:"exit_code,omitempty"`
	PRURL        string     `json:"pr_url,omitempty"`
	CreatedAt    time.Time  `json:"created_at"`
	UpdatedAt    time.Time  `json:"updated_at"`
	FinishedAt   *time.Time `json:"finished_at,omitempty"`
}

type TerminalSession struct {
	ID         string     `json:"id"`
	SessionID  string     `json:"session_id"`
	Title      string     `json:"title"`
	Cwd        string     `json:"cwd"`
	Status     string     `json:"status"`
	PID        int        `json:"pid,omitempty"`
	ExitCode   *int       `json:"exit_code,omitempty"`
	CreatedAt  time.Time  `json:"created_at"`
	UpdatedAt  time.Time  `json:"updated_at"`
	FinishedAt *time.Time `json:"finished_at,omitempty"`
}

type QueuedMessage struct {
	ID        string    `json:"id"`
	SessionID string    `json:"session_id"`
	Text      string    `json:"text"`
	Status    string    `json:"status"`
	Priority  int       `json:"priority"`
	CreatedAt time.Time `json:"created_at"`
	UpdatedAt time.Time `json:"updated_at"`
}

type Store struct {
	db *sql.DB
}

func NewStore(dataDir string) (*Store, error) {
	dbPath := filepath.Join(dataDir, "openade.sqlite3")
	db, err := sql.Open("sqlite3", dbPath+"?_busy_timeout=5000&_journal_mode=WAL&_foreign_keys=on")
	if err != nil {
		return nil, fmt.Errorf("open sqlite: %w", err)
	}
	db.SetMaxOpenConns(1)
	s := &Store{db: db}
	if err := s.migrate(); err != nil {
		db.Close()
		return nil, err
	}
	return s, nil
}

func (s *Store) Close() error { return s.db.Close() }

func (s *Store) migrate() error {
	_, err := s.db.Exec(`
CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  prompt TEXT NOT NULL DEFAULT '',
  agent TEXT NOT NULL,
  repo_root TEXT NOT NULL,
  worktree_path TEXT NOT NULL,
  branch TEXT NOT NULL,
  base_branch TEXT NOT NULL DEFAULT 'main',
  ticket_key TEXT NOT NULL DEFAULT '',
  ticket_url TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL,
  pid INTEGER NOT NULL DEFAULT 0,
  exit_code INTEGER,
  pr_url TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  finished_at TEXT
);
CREATE INDEX IF NOT EXISTS sessions_updated_idx ON sessions(updated_at DESC);
CREATE INDEX IF NOT EXISTS sessions_repo_idx ON sessions(repo_root, updated_at DESC);
CREATE INDEX IF NOT EXISTS sessions_ticket_idx ON sessions(ticket_key) WHERE ticket_key <> '';
CREATE TABLE IF NOT EXISTS terminals (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  cwd TEXT NOT NULL,
  status TEXT NOT NULL,
  pid INTEGER NOT NULL DEFAULT 0,
  exit_code INTEGER,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  finished_at TEXT
);
CREATE INDEX IF NOT EXISTS terminals_session_idx ON terminals(session_id, created_at);
CREATE TABLE IF NOT EXISTS message_queue (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  text TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'queued',
  priority INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS message_queue_session_idx ON message_queue(session_id, status, priority DESC, created_at);
`)
	if err != nil {
		return fmt.Errorf("migrate sqlite: %w", err)
	}
	if _, alterErr := s.db.Exec(`ALTER TABLE sessions ADD COLUMN mode TEXT NOT NULL DEFAULT 'chat'`); alterErr != nil && !strings.Contains(alterErr.Error(), "duplicate column") {
		return fmt.Errorf("add session mode: %w", alterErr)
	}
	if _, resetErr := s.db.Exec(`UPDATE message_queue SET status='queued', updated_at=? WHERE status='dispatching'`, encodeTime(time.Now().UTC())); resetErr != nil {
		return resetErr
	}
	_, err = s.db.Exec(`UPDATE sessions SET status = 'interrupted', pid = 0,
updated_at = ? WHERE status IN ('starting', 'running', 'waiting')`, time.Now().UTC().Format(time.RFC3339Nano))
	if err == nil {
		_, err = s.db.Exec(`UPDATE terminals SET status = 'interrupted', pid = 0,
updated_at = ? WHERE status IN ('starting', 'running')`, time.Now().UTC().Format(time.RFC3339Nano))
	}
	return err
}

func (s *Store) EnqueueMessage(message QueuedMessage) error {
	_, err := s.db.Exec(`INSERT INTO message_queue(id,session_id,text,status,priority,created_at,updated_at)
VALUES(?,?,?,?,?,?,?)`, message.ID, message.SessionID, message.Text, message.Status, message.Priority,
		encodeTime(message.CreatedAt), encodeTime(message.UpdatedAt))
	return err
}

func (s *Store) ListQueuedMessages(sessionID string) ([]QueuedMessage, error) {
	rows, err := s.db.Query(`SELECT id,session_id,text,status,priority,created_at,updated_at
FROM message_queue WHERE session_id=? ORDER BY CASE status WHEN 'dispatching' THEN 0 ELSE 1 END, priority DESC, created_at`, sessionID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	messages := []QueuedMessage{}
	for rows.Next() {
		message, err := scanQueuedMessage(rows)
		if err != nil {
			return nil, err
		}
		messages = append(messages, message)
	}
	return messages, rows.Err()
}

func (s *Store) PromoteQueuedMessage(sessionID, messageID string) error {
	result, err := s.db.Exec(`UPDATE message_queue SET priority=(SELECT COALESCE(MAX(priority),0)+1 FROM message_queue WHERE session_id=?), updated_at=?
WHERE id=? AND session_id=? AND status='queued'`, sessionID, encodeTime(time.Now().UTC()), messageID, sessionID)
	if err != nil {
		return err
	}
	return requireAffected(result, "queued message")
}

func (s *Store) DeleteQueuedMessage(sessionID, messageID string) error {
	result, err := s.db.Exec(`DELETE FROM message_queue WHERE id=? AND session_id=? AND status='queued'`, messageID, sessionID)
	if err != nil {
		return err
	}
	return requireAffected(result, "queued message")
}

func (s *Store) ClaimNextQueuedMessage(sessionID string) (QueuedMessage, error) {
	tx, err := s.db.Begin()
	if err != nil {
		return QueuedMessage{}, err
	}
	defer tx.Rollback()
	message, err := scanQueuedMessage(tx.QueryRow(`SELECT id,session_id,text,status,priority,created_at,updated_at
FROM message_queue WHERE session_id=? AND status='queued' ORDER BY priority DESC, created_at LIMIT 1`, sessionID))
	if err != nil {
		return QueuedMessage{}, err
	}
	message.Status = "dispatching"
	message.UpdatedAt = time.Now().UTC()
	result, err := tx.Exec(`UPDATE message_queue SET status='dispatching', updated_at=? WHERE id=? AND status='queued'`, encodeTime(message.UpdatedAt), message.ID)
	if err != nil {
		return QueuedMessage{}, err
	}
	if err := requireAffected(result, "queued message"); err != nil {
		return QueuedMessage{}, err
	}
	if err := tx.Commit(); err != nil {
		return QueuedMessage{}, err
	}
	return message, nil
}

func (s *Store) ReleaseQueuedMessage(messageID string) error {
	_, err := s.db.Exec(`UPDATE message_queue SET status='queued', updated_at=? WHERE id=? AND status='dispatching'`, encodeTime(time.Now().UTC()), messageID)
	return err
}

func (s *Store) CompleteQueuedMessage(messageID string) error {
	_, err := s.db.Exec(`DELETE FROM message_queue WHERE id=? AND status='dispatching'`, messageID)
	return err
}

func (s *Store) CreateSession(session Session) error {
	_, err := s.db.Exec(`INSERT INTO sessions
(id,title,prompt,agent,mode,repo_root,worktree_path,branch,base_branch,ticket_key,ticket_url,status,pid,created_at,updated_at)
VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)`, session.ID, session.Title, session.Prompt, session.Agent, session.Mode,
		session.RepoRoot, session.WorktreePath, session.Branch, session.BaseBranch, session.TicketKey,
		session.TicketURL, session.Status, session.PID, encodeTime(session.CreatedAt), encodeTime(session.UpdatedAt))
	return err
}

func (s *Store) UpdateRuntime(id, status string, pid int, exitCode *int) error {
	now := time.Now().UTC()
	var finished any
	if status == "completed" || status == "failed" || status == "stopped" {
		finished = encodeTime(now)
	}
	_, err := s.db.Exec(`UPDATE sessions SET status=?, pid=?, exit_code=?, updated_at=?,
finished_at=COALESCE(?, finished_at) WHERE id=?`, status, pid, exitCode, encodeTime(now), finished, id)
	return err
}

func (s *Store) SetPR(id, url string) error {
	_, err := s.db.Exec(`UPDATE sessions SET pr_url=?, updated_at=? WHERE id=?`, url, encodeTime(time.Now().UTC()), id)
	return err
}

func (s *Store) ListSessions() ([]Session, error) {
	rows, err := s.db.Query(`SELECT id,title,prompt,agent,mode,repo_root,worktree_path,branch,base_branch,
ticket_key,ticket_url,status,pid,exit_code,pr_url,created_at,updated_at,finished_at
FROM sessions ORDER BY updated_at DESC`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var sessions []Session
	for rows.Next() {
		session, err := scanSession(rows)
		if err != nil {
			return nil, err
		}
		sessions = append(sessions, session)
	}
	return sessions, rows.Err()
}

func (s *Store) GetSession(id string) (Session, error) {
	row := s.db.QueryRow(`SELECT id,title,prompt,agent,mode,repo_root,worktree_path,branch,base_branch,
ticket_key,ticket_url,status,pid,exit_code,pr_url,created_at,updated_at,finished_at
FROM sessions WHERE id=?`, id)
	return scanSession(row)
}

func (s *Store) ListProjects() ([]string, error) {
	rows, err := s.db.Query(`SELECT repo_root FROM sessions GROUP BY repo_root ORDER BY MAX(updated_at) DESC`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	projects := []string{}
	for rows.Next() {
		var project string
		if err := rows.Scan(&project); err != nil {
			return nil, err
		}
		projects = append(projects, project)
	}
	return projects, rows.Err()
}

func (s *Store) CreateTerminal(terminal TerminalSession) error {
	_, err := s.db.Exec(`INSERT INTO terminals
(id,session_id,title,cwd,status,pid,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?)`,
		terminal.ID, terminal.SessionID, terminal.Title, terminal.Cwd, terminal.Status,
		terminal.PID, encodeTime(terminal.CreatedAt), encodeTime(terminal.UpdatedAt))
	return err
}

func (s *Store) UpdateTerminalRuntime(id, status string, pid int, exitCode *int) error {
	now := time.Now().UTC()
	var finished any
	if status == "completed" || status == "failed" || status == "stopped" {
		finished = encodeTime(now)
	}
	_, err := s.db.Exec(`UPDATE terminals SET status=?, pid=?, exit_code=?, updated_at=?,
finished_at=COALESCE(?, finished_at) WHERE id=?`, status, pid, exitCode, encodeTime(now), finished, id)
	return err
}

func (s *Store) ListTerminals(sessionID string) ([]TerminalSession, error) {
	rows, err := s.db.Query(`SELECT id,session_id,title,cwd,status,pid,exit_code,created_at,updated_at,finished_at
FROM terminals WHERE session_id=? ORDER BY created_at`, sessionID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	terminals := []TerminalSession{}
	for rows.Next() {
		terminal, err := scanTerminal(rows)
		if err != nil {
			return nil, err
		}
		terminals = append(terminals, terminal)
	}
	return terminals, rows.Err()
}

func (s *Store) GetTerminal(id string) (TerminalSession, error) {
	row := s.db.QueryRow(`SELECT id,session_id,title,cwd,status,pid,exit_code,created_at,updated_at,finished_at
FROM terminals WHERE id=?`, id)
	return scanTerminal(row)
}

type scanner interface{ Scan(...any) error }

func scanSession(row scanner) (Session, error) {
	var session Session
	var created, updated string
	var finished sql.NullString
	var exitCode sql.NullInt64
	err := row.Scan(&session.ID, &session.Title, &session.Prompt, &session.Agent, &session.Mode, &session.RepoRoot,
		&session.WorktreePath, &session.Branch, &session.BaseBranch, &session.TicketKey, &session.TicketURL,
		&session.Status, &session.PID, &exitCode, &session.PRURL, &created, &updated, &finished)
	if err != nil {
		return session, err
	}
	session.CreatedAt, _ = time.Parse(time.RFC3339Nano, created)
	session.UpdatedAt, _ = time.Parse(time.RFC3339Nano, updated)
	if finished.Valid {
		t, _ := time.Parse(time.RFC3339Nano, finished.String)
		session.FinishedAt = &t
	}
	if exitCode.Valid {
		code := int(exitCode.Int64)
		session.ExitCode = &code
	}
	return session, nil
}

func scanTerminal(row scanner) (TerminalSession, error) {
	var terminal TerminalSession
	var created, updated string
	var finished sql.NullString
	var exitCode sql.NullInt64
	err := row.Scan(&terminal.ID, &terminal.SessionID, &terminal.Title, &terminal.Cwd, &terminal.Status,
		&terminal.PID, &exitCode, &created, &updated, &finished)
	if err != nil {
		return terminal, err
	}
	terminal.CreatedAt, _ = time.Parse(time.RFC3339Nano, created)
	terminal.UpdatedAt, _ = time.Parse(time.RFC3339Nano, updated)
	if finished.Valid {
		t, _ := time.Parse(time.RFC3339Nano, finished.String)
		terminal.FinishedAt = &t
	}
	if exitCode.Valid {
		code := int(exitCode.Int64)
		terminal.ExitCode = &code
	}
	return terminal, nil
}

func scanQueuedMessage(row scanner) (QueuedMessage, error) {
	var message QueuedMessage
	var created, updated string
	err := row.Scan(&message.ID, &message.SessionID, &message.Text, &message.Status, &message.Priority, &created, &updated)
	if err != nil {
		return message, err
	}
	message.CreatedAt, _ = time.Parse(time.RFC3339Nano, created)
	message.UpdatedAt, _ = time.Parse(time.RFC3339Nano, updated)
	return message, nil
}

func requireAffected(result sql.Result, label string) error {
	affected, err := result.RowsAffected()
	if err != nil {
		return err
	}
	if affected == 0 {
		return fmt.Errorf("%s was not found", label)
	}
	return nil
}

func encodeTime(t time.Time) string { return t.UTC().Format(time.RFC3339Nano) }

func IsNotFound(err error) bool { return errors.Is(err, sql.ErrNoRows) }

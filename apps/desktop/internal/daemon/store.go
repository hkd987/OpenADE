package daemon

import (
	"database/sql"
	"errors"
	"fmt"
	"path/filepath"
	"time"

	_ "github.com/mattn/go-sqlite3"
)

type Session struct {
	ID           string     `json:"id"`
	Title        string     `json:"title"`
	Prompt       string     `json:"prompt"`
	Agent        string     `json:"agent"`
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
`)
	if err != nil {
		return fmt.Errorf("migrate sqlite: %w", err)
	}
	_, err = s.db.Exec(`UPDATE sessions SET status = 'interrupted', pid = 0,
updated_at = ? WHERE status IN ('starting', 'running', 'waiting')`, time.Now().UTC().Format(time.RFC3339Nano))
	return err
}

func (s *Store) CreateSession(session Session) error {
	_, err := s.db.Exec(`INSERT INTO sessions
(id,title,prompt,agent,repo_root,worktree_path,branch,base_branch,ticket_key,ticket_url,status,pid,created_at,updated_at)
VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)`, session.ID, session.Title, session.Prompt, session.Agent,
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
	rows, err := s.db.Query(`SELECT id,title,prompt,agent,repo_root,worktree_path,branch,base_branch,
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
	row := s.db.QueryRow(`SELECT id,title,prompt,agent,repo_root,worktree_path,branch,base_branch,
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

type scanner interface{ Scan(...any) error }

func scanSession(row scanner) (Session, error) {
	var session Session
	var created, updated string
	var finished sql.NullString
	var exitCode sql.NullInt64
	err := row.Scan(&session.ID, &session.Title, &session.Prompt, &session.Agent, &session.RepoRoot,
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

func encodeTime(t time.Time) string { return t.UTC().Format(time.RFC3339Nano) }

func IsNotFound(err error) bool { return errors.Is(err, sql.ErrNoRows) }

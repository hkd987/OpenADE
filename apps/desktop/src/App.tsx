import { useCallback, useEffect, useState } from "react";
import {
  DaemonConfig,
  getConfig,
  listSessions,
  projectName,
  SessionMeta,
} from "./api";
import { CommandPalette } from "./components/CommandPalette";
import { NewSessionForm } from "./components/NewSessionForm";
import { Onboarding } from "./components/Onboarding";
import { ProjectsView } from "./components/ProjectsView";
import { SessionCard } from "./components/SessionCard";
import { SessionDetail } from "./components/SessionDetail";

const POLL_MS = 2000;

export default function App() {
  const [sessions, setSessions] = useState<SessionMeta[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [daemonError, setDaemonError] = useState<string | null>(null);
  const [showForm, setShowForm] = useState(false);
  const [formRepo, setFormRepo] = useState<string | undefined>(undefined);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [config, setConfig] = useState<DaemonConfig | null>(null);
  const [view, setView] = useState<"sessions" | "projects">("sessions");
  const [showPalette, setShowPalette] = useState(false);
  const [showSettings, setShowSettings] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const { sessions } = await listSessions();
      setSessions(sessions);
      setDaemonError(null);
    } catch (err) {
      setDaemonError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  const refreshConfig = useCallback(async () => {
    // First-run detection; an unreachable daemon just skips onboarding
    // (the error banner already covers that case).
    try {
      setConfig(await getConfig());
    } catch {
      setConfig(null);
    }
  }, []);

  useEffect(() => {
    void refresh();
    void refreshConfig();
    const timer = setInterval(() => void refresh(), POLL_MS);
    return () => clearInterval(timer);
  }, [refresh, refreshConfig]);

  // ⌘K / Ctrl+K opens the command palette from anywhere.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setShowPalette((v) => !v);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const openForm = (repo?: string) => {
    setFormRepo(repo);
    setShowForm(true);
    setView("sessions");
  };

  const selectedSession = sessions.find((s) => s.id === selected) ?? null;

  // Sessions grouped by project (repository), in the order projects appear.
  const projects: { repoRoot: string; sessions: SessionMeta[] }[] = [];
  for (const session of sessions) {
    const group = projects.find((p) => p.repoRoot === session.repo_root);
    if (group) {
      group.sessions.push(session);
    } else {
      projects.push({ repoRoot: session.repo_root, sessions: [session] });
    }
  }

  return (
    <div className="app">
      {config !== null && !config.onboarded && (
        <Onboarding config={config} onDone={() => void refreshConfig()} />
      )}
      {showSettings && config !== null && (
        <Onboarding
          mode="settings"
          config={config}
          onDone={() => {
            setShowSettings(false);
            void refreshConfig();
          }}
          onClose={() => setShowSettings(false)}
        />
      )}
      {showPalette && (
        <CommandPalette
          sessions={sessions}
          onSelect={(id) => {
            setSelected(id);
            setView("sessions");
          }}
          onNewSession={() => openForm(undefined)}
          onClose={() => setShowPalette(false)}
        />
      )}
      <header className="app-header">
        <h1>OpenADE</h1>
        <span
          className={`daemon-health ${daemonError === null ? "ok" : "err"}`}
          title={
            daemonError === null ? "Daemon connected" : "Daemon unreachable"
          }
          data-testid="daemon-health"
        />
        <nav className="view-nav" aria-label="Views">
          <button
            className={`view-pill ${view === "sessions" ? "active" : ""}`}
            onClick={() => setView("sessions")}
            data-testid="view-sessions"
          >
            Sessions
          </button>
          <button
            className={`view-pill ${view === "projects" ? "active" : ""}`}
            onClick={() => setView("projects")}
            data-testid="view-projects"
          >
            Projects
          </button>
        </nav>
        <span className="header-spacer" />
        <button
          className="kbd-hint"
          title="Command palette (⌘K / Ctrl+K)"
          onClick={() => setShowPalette(true)}
          data-testid="palette-button"
        >
          ⌘K
        </button>
        <button
          className="settings-button"
          title="Settings"
          onClick={() => setShowSettings(true)}
          data-testid="settings-button"
        >
          ⚙
        </button>
        <button
          className="new-session-button"
          onClick={() => {
            setFormRepo(undefined);
            setShowForm((v) => !v);
          }}
          data-testid="toggle-new-session"
        >
          {showForm ? "Close" : "New session"}
        </button>
      </header>

      {daemonError !== null && (
        <div className="daemon-error" data-testid="daemon-error">
          Cannot reach openade-daemon — is it running? <code>{daemonError}</code>
        </div>
      )}

      {showForm && (
        <NewSessionForm
          initialRepo={formRepo}
          onCreated={(session) => {
            setShowForm(false);
            setSelected(session.id);
            setView("sessions");
            void refresh();
          }}
          onClose={() => setShowForm(false)}
        />
      )}

      {view === "projects" ? (
        <ProjectsView
          projects={projects}
          onNewSession={(repo) => openForm(repo)}
          onOpenProject={() => setView("sessions")}
        />
      ) : (
        <main className="layout">
          <section
            className="session-grid"
            aria-label="Sessions"
            data-testid="session-grid"
          >
            {sessions.length === 0 && daemonError === null && (
              <p className="empty" data-testid="empty-grid">
                No sessions yet — press “New session” to launch one.
              </p>
            )}
            {projects.map((project) => {
              const isCollapsed = collapsed.has(project.repoRoot);
              return (
                <div key={project.repoRoot} className="project-group">
                  <div className="project-group-bar">
                    <button
                      type="button"
                      className="project-group-header"
                      title={project.repoRoot}
                      data-testid="project-group-header"
                      aria-expanded={!isCollapsed}
                      onClick={() =>
                        setCollapsed((prev) => {
                          const next = new Set(prev);
                          if (next.has(project.repoRoot)) {
                            next.delete(project.repoRoot);
                          } else {
                            next.add(project.repoRoot);
                          }
                          return next;
                        })
                      }
                    >
                      <span className="chevron">
                        {isCollapsed ? "▸" : "▾"}
                      </span>
                      {projectName(project.repoRoot)}
                    </button>
                    <button
                      type="button"
                      className="project-add"
                      title={`New session in ${projectName(project.repoRoot)}`}
                      data-testid="project-add"
                      onClick={() => openForm(project.repoRoot)}
                    >
                      +
                    </button>
                  </div>
                  {!isCollapsed &&
                    project.sessions.map((session) => (
                      <SessionCard
                        key={session.id}
                        session={session}
                        selected={session.id === selected}
                        onSelect={() => setSelected(session.id)}
                      />
                    ))}
                </div>
              );
            })}
          </section>

          <section className="terminal-pane" aria-label="Session detail">
            {selectedSession ? (
              <SessionDetail
                session={selectedSession}
                onChanged={(selectId) => {
                  if (selectId !== undefined) {
                    setSelected(selectId);
                  }
                  void refresh();
                }}
              />
            ) : (
              <p className="empty">Select a session to attach.</p>
            )}
          </section>
        </main>
      )}
    </div>
  );
}

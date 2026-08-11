import { useCallback, useEffect, useState } from "react";
import { listSessions, projectName, SessionMeta } from "./api";
import { NewSessionForm } from "./components/NewSessionForm";
import { SessionCard } from "./components/SessionCard";
import { SessionDetail } from "./components/SessionDetail";

const POLL_MS = 2000;

export default function App() {
  const [sessions, setSessions] = useState<SessionMeta[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [daemonError, setDaemonError] = useState<string | null>(null);
  const [showForm, setShowForm] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const { sessions } = await listSessions();
      setSessions(sessions);
      setDaemonError(null);
    } catch (err) {
      setDaemonError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), POLL_MS);
    return () => clearInterval(timer);
  }, [refresh]);

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
      <header className="app-header">
        <h1>OpenADE</h1>
        <span className="tagline">open agentic development environment</span>
        <button
          className="new-session-button"
          onClick={() => setShowForm((v) => !v)}
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
          onCreated={(session) => {
            setShowForm(false);
            setSelected(session.id);
            void refresh();
          }}
          onClose={() => setShowForm(false)}
        />
      )}

      <main className="layout">
        <section className="session-grid" aria-label="Sessions" data-testid="session-grid">
          {sessions.length === 0 && daemonError === null && (
            <p className="empty" data-testid="empty-grid">
              No sessions yet — press “New session” to launch one.
            </p>
          )}
          {projects.map((project) => (
            <div key={project.repoRoot} className="project-group">
              <div
                className="project-group-header"
                title={project.repoRoot}
                data-testid="project-group-header"
              >
                {projectName(project.repoRoot)}
              </div>
              {project.sessions.map((session) => (
                <SessionCard
                  key={session.id}
                  session={session}
                  selected={session.id === selected}
                  onSelect={() => setSelected(session.id)}
                />
              ))}
            </div>
          ))}
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
    </div>
  );
}

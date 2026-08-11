import { useCallback, useEffect, useState } from "react";
import { listSessions, SessionMeta } from "./api";
import { SessionCard } from "./components/SessionCard";
import { TerminalView } from "./components/TerminalView";

const POLL_MS = 2000;

export default function App() {
  const [sessions, setSessions] = useState<SessionMeta[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [daemonError, setDaemonError] = useState<string | null>(null);

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

  return (
    <div className="app">
      <header className="app-header">
        <h1>OpenADE</h1>
        <span className="tagline">open agentic development environment</span>
      </header>

      {daemonError !== null && (
        <div className="daemon-error">
          Cannot reach openade-daemon — is it running? <code>{daemonError}</code>
        </div>
      )}

      <main className="layout">
        <section className="session-grid" aria-label="Sessions">
          {sessions.length === 0 && daemonError === null && (
            <p className="empty">
              No sessions yet. Launch one via the daemon API, e.g.{" "}
              <code>POST /sessions</code>.
            </p>
          )}
          {sessions.map((session) => (
            <SessionCard
              key={session.id}
              session={session}
              selected={session.id === selected}
              onSelect={() => setSelected(session.id)}
            />
          ))}
        </section>

        <section className="terminal-pane" aria-label="Terminal">
          {selectedSession ? (
            <TerminalView key={selectedSession.id} session={selectedSession} />
          ) : (
            <p className="empty">Select a session to attach its terminal.</p>
          )}
        </section>
      </main>
    </div>
  );
}

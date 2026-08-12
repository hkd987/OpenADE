import { useCallback, useEffect, useState } from "react";
import {
  getWorkspaceSession,
  harnessLabel,
  Harness,
  HARNESSES,
  listWorkspaceSessions,
  pickupSession,
  SessionMeta,
  splitEntityRef,
  timeAgo,
  WorkspaceSession,
  WorkspaceSessionDetail,
} from "../api";

/**
 * The Team view: shared sessions on the configured workspace server,
 * proxied through the daemon (the member token never reaches the browser).
 *
 * Every listed session can be opened read-only (artifact + transcript) and
 * picked up as a *new local session* in any harness — the daemon renders
 * the harness-neutral record into the chosen harness's own conventions.
 */
export function TeamView({
  configured,
  repos,
  onPickedUp,
}: {
  /** Whether the daemon has a workspace server configured. */
  configured: boolean;
  /** Known local repositories, to prefill the pickup target. */
  repos: string[];
  onPickedUp: (session: SessionMeta) => void;
}) {
  const [sessions, setSessions] = useState<WorkspaceSession[]>([]);
  const [detail, setDetail] = useState<WorkspaceSessionDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [harness, setHarness] = useState<Harness>("claude-code");
  const [repo, setRepo] = useState(repos[0] ?? "");
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const { sessions } = await listWorkspaceSessions();
      setSessions(sessions);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    if (configured) {
      void refresh();
    }
  }, [configured, refresh]);

  if (!configured) {
    return (
      <main className="team-view" data-testid="team-unconfigured">
        <p className="empty">
          Multiplayer is off — point OpenADE at a self-hosted{" "}
          <code>openade-server</code> (URL, member token, and workspace id) in
          Settings to share sessions with your team.
        </p>
      </main>
    );
  }

  const open = (id: number) => {
    getWorkspaceSession(id)
      .then(setDetail)
      .catch((err) =>
        setError(err instanceof Error ? err.message : String(err)),
      );
  };

  const pickup = async (workspaceSessionId: number) => {
    setBusy(true);
    setError(null);
    try {
      const meta = await pickupSession({
        workspace_session_id: workspaceSessionId,
        harness,
        repo_root: repo,
      });
      onPickedUp(meta);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setBusy(false);
    }
  };

  return (
    <main className="team-view" data-testid="team-view">
      {error !== null && (
        <div className="form-error" role="alert" data-testid="team-error">
          {error}
        </div>
      )}

      {detail === null ? (
        <section className="team-list" aria-label="Shared sessions">
          {sessions.length === 0 && error === null && (
            <p className="empty" data-testid="team-empty">
              No shared sessions yet — open a session and press “Share” to
              publish it to the team workspace.
            </p>
          )}
          {sessions.map((s) => (
            <button
              key={s.id}
              type="button"
              className="team-row"
              data-testid="team-row"
              onClick={() => open(s.id)}
            >
              <div className="team-row-head">
                <strong>{s.title}</strong>
                <span className="team-harness">
                  {harnessLabel(s.harness as Harness)}
                </span>
                {s.entity_ref != null && (
                  <span className="memory-chip" title={s.entity_ref}>
                    {splitEntityRef(s.entity_ref).rest}
                  </span>
                )}
              </div>
              <div className="team-row-meta">
                shared by <strong>{s.shared_by}</strong>
                {timeAgo(s.uploaded_at) !== "" && (
                  <> · {timeAgo(s.uploaded_at)}</>
                )}
              </div>
              <p className="team-summary">{s.summary}</p>
            </button>
          ))}
        </section>
      ) : (
        <section className="team-detail" data-testid="team-detail">
          <div className="file-viewer-bar">
            <strong>{detail.session.title}</strong>
            <button
              type="button"
              className="secondary"
              onClick={() => setDetail(null)}
              data-testid="team-back"
            >
              Back
            </button>
          </div>

          <div className="team-pickup">
            <select
              value={harness}
              onChange={(e) => setHarness(e.target.value as Harness)}
              title="Harness to pick the session up in"
              data-testid="pickup-harness"
            >
              {HARNESSES.map((h) => (
                <option key={h.id} value={h.id}>
                  {h.label}
                </option>
              ))}
            </select>
            <input
              value={repo}
              onChange={(e) => setRepo(e.target.value)}
              placeholder="/path/to/your/clone"
              title="Local repository to pick the session up in"
              data-testid="pickup-repo"
            />
            <button
              disabled={busy || repo === ""}
              onClick={() => void pickup(detail.session.id)}
              data-testid="pickup-button"
            >
              {busy ? "Picking up…" : "Pick up"}
            </button>
          </div>

          <pre className="team-markdown" data-testid="team-markdown">
            {detail.markdown}
          </pre>

          <h4 className="team-transcript-title">Transcript</h4>
          <ul className="team-transcript" data-testid="team-transcript">
            {detail.events.map((e, i) => (
              <li key={i}>
                <span className={`event-kind ${e.kind}`}>{e.kind}</span>
                {e.payload?.text !== undefined && <code>{e.payload.text}</code>}
              </li>
            ))}
          </ul>
        </section>
      )}
    </main>
  );
}

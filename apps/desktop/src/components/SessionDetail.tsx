import { useEffect, useState } from "react";
import {
  ArtifactInfo,
  getDiff,
  getFiles,
  handoffSession,
  HARNESSES,
  Harness,
  killSession,
  publishArtifact,
  SessionMeta,
  timeAgo,
} from "../api";
import { TerminalView } from "./TerminalView";

type Tab = "terminal" | "diff" | "files";

export function SessionDetail({
  session,
  onChanged,
}: {
  session: SessionMeta;
  onChanged: (selectId?: string) => void;
}) {
  const [tab, setTab] = useState<Tab>("terminal");
  const [diff, setDiff] = useState("");
  const [files, setFiles] = useState<string[]>([]);
  const [artifact, setArtifact] = useState<ArtifactInfo | null>(null);
  const [handoffTo, setHandoffTo] = useState<Harness>("gemini-cli");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setArtifact(null);
    setError(null);
    setTab("terminal");
  }, [session.id]);

  useEffect(() => {
    if (tab === "diff") {
      getDiff(session.id)
        .then(({ diff }) => setDiff(diff))
        .catch((e) => setError(String(e)));
    }
    if (tab === "files") {
      getFiles(session.id)
        .then(({ files }) => setFiles(files))
        .catch((e) => setError(String(e)));
    }
  }, [tab, session.id]);

  const act = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="session-detail" data-testid="session-detail">
      <div className="detail-header">
        <div className="detail-title">
          <strong>{session.title}</strong>
          <span className="worktree">{session.worktree_path}</span>
          {timeAgo(session.created_at) !== "" && (
            <span className="session-runtime" data-testid="session-runtime">
              started {timeAgo(session.created_at)}
            </span>
          )}
        </div>
        <div className="detail-actions">
          <select
            value={handoffTo}
            onChange={(e) => setHandoffTo(e.target.value as Harness)}
            title="Harness to hand off to"
            data-testid="handoff-target"
          >
            {HARNESSES.filter((h) => h.id !== session.harness).map((h) => (
              <option key={h.id} value={h.id}>
                {h.label}
              </option>
            ))}
          </select>
          <button
            disabled={busy}
            onClick={() =>
              act(async () => {
                const next = await handoffSession(session.id, handoffTo);
                onChanged(next.id);
              })
            }
            data-testid="handoff-button"
          >
            Handoff
          </button>
          <button
            disabled={busy}
            onClick={() =>
              act(async () => {
                setArtifact(await publishArtifact(session.id));
                onChanged();
              })
            }
            data-testid="artifact-button"
          >
            Artifact
          </button>
          <button
            disabled={busy}
            className="danger"
            onClick={() =>
              act(async () => {
                await killSession(session.id);
                onChanged();
              })
            }
            data-testid="kill-button"
          >
            Kill
          </button>
        </div>
      </div>

      {error !== null && (
        <div className="form-error" role="alert">
          {error}
        </div>
      )}
      {artifact !== null && (
        <div className="artifact-banner" data-testid="artifact-banner">
          Knowledge artifact committed to branch <code>{artifact.branch}</code>{" "}
          (<code>{artifact.file}</code>) — review and merge to publish.
          {artifact.shared_repo && (
            <>
              {" "}
              Also pushed to team memory{" "}
              <a
                href={`https://github.com/${artifact.shared_repo}/blob/HEAD/${artifact.shared_path}`}
                target="_blank"
                rel="noreferrer"
                data-testid="shared-memory-link"
              >
                {artifact.shared_repo}
              </a>
              .
            </>
          )}
        </div>
      )}

      <nav className="tabs" aria-label="Session views">
        {(["terminal", "diff", "files"] as Tab[]).map((t) => (
          <button
            key={t}
            className={tab === t ? "tab active" : "tab"}
            onClick={() => setTab(t)}
            data-testid={`tab-${t}`}
          >
            {t}
          </button>
        ))}
      </nav>

      {tab === "terminal" && <TerminalView key={session.id} session={session} />}
      {tab === "diff" && (
        <pre className="diff-view" data-testid="diff-view">
          {diff === "" ? "No changes against the task's base commit yet." : diff}
        </pre>
      )}
      {tab === "files" && (
        <ul className="file-list" data-testid="file-list">
          {files.map((f) => (
            <li key={f}>
              <code>{f}</code>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

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
  getFileContent,
  getSkills,
  SessionMeta,
  shareSession,
  SkillInfo,
  timeAgo,
  WorkspaceSession,
} from "../api";
import { TerminalView } from "./TerminalView";

type Tab = "terminal" | "diff" | "files" | "rules" | "skills";

const RULES_FILES = ["CLAUDE.md", "AGENTS.md", "GEMINI.md", ".openade/rules.md"];

export function SessionDetail({
  session,
  onChanged,
  workspaceConfigured = false,
}: {
  session: SessionMeta;
  onChanged: (selectId?: string) => void;
  /** Whether a multiplayer workspace server is configured (Share button). */
  workspaceConfigured?: boolean;
}) {
  const [tab, setTab] = useState<Tab>("terminal");
  const [diff, setDiff] = useState("");
  const [files, setFiles] = useState<string[]>([]);
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const [viewing, setViewing] = useState<{ path: string; content: string } | null>(null);
  const [artifact, setArtifact] = useState<ArtifactInfo | null>(null);
  const [shared, setShared] = useState<WorkspaceSession | null>(null);
  const [handoffTo, setHandoffTo] = useState<Harness>("gemini-cli");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setArtifact(null);
    setShared(null);
    setError(null);
    setViewing(null);
    setTab("terminal");
  }, [session.id]);

  useEffect(() => {
    if (tab === "diff") {
      getDiff(session.id)
        .then(({ diff }) => setDiff(diff))
        .catch((e) => setError(String(e)));
    }
    if (tab === "files" || tab === "rules") {
      getFiles(session.id)
        .then(({ files }) => setFiles(files))
        .catch((e) => setError(String(e)));
    }
    if (tab === "skills") {
      getSkills(session.id)
        .then(({ skills }) => setSkills(skills))
        .catch((e) => setError(String(e)));
    }
    setViewing(null);
  }, [tab, session.id]);

  const view = (path: string) => {
    getFileContent(session.id, path)
      .then(setViewing)
      .catch((e) => setError(String(e)));
  };

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
          {workspaceConfigured && (
            <button
              disabled={busy}
              onClick={() =>
                act(async () => {
                  setShared(await shareSession(session.id));
                })
              }
              data-testid="share-button"
            >
              Share
            </button>
          )}
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

      {shared !== null && (
        <div className="artifact-banner" data-testid="share-banner">
          Shared to the team workspace as session <code>#{shared.id}</code> —
          teammates can browse it in the Team view and pick it up in any
          harness.
        </div>
      )}

      <nav className="tabs" aria-label="Session views">
        {(["terminal", "diff", "files", "rules", "skills"] as Tab[]).map((t) => (
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
      {tab === "files" && viewing === null && (
        <ul className="file-list" data-testid="file-list">
          {files.map((f) => (
            <li key={f}>
              <button type="button" className="file-row" onClick={() => view(f)}>
                <code>{f}</code>
              </button>
            </li>
          ))}
        </ul>
      )}
      {tab === "rules" && viewing === null && (
        <ul className="file-list" data-testid="rules-list">
          {files.filter((f) => RULES_FILES.includes(f)).map((f) => (
            <li key={f}>
              <button type="button" className="file-row" onClick={() => view(f)}>
                <code>{f}</code>
              </button>
            </li>
          ))}
          {files.filter((f) => RULES_FILES.includes(f)).length === 0 && (
            <li className="empty">
              No rules files — add <code>.openade/rules.md</code> to the
              repository and they materialize per harness.
            </li>
          )}
        </ul>
      )}
      {tab === "skills" && viewing === null && (
        <ul className="file-list" data-testid="skills-list">
          {skills.map((s) => (
            <li key={s.path}>
              <button
                type="button"
                className="file-row"
                onClick={() => view(s.path)}
              >
                <strong>{s.name}</strong>
                <span className="skill-desc">{s.description}</span>
              </button>
            </li>
          ))}
          {skills.length === 0 && (
            <li className="empty">
              No skills discovered — add <code>.claude/skills/&lt;name&gt;/SKILL.md</code>{" "}
              or <code>.openade/skills/*.md</code>.
            </li>
          )}
        </ul>
      )}
      {tab !== "terminal" && tab !== "diff" && viewing !== null && (
        <div className="file-viewer" data-testid="file-viewer">
          <div className="file-viewer-bar">
            <code>{viewing.path}</code>
            <button
              type="button"
              className="secondary"
              onClick={() => setViewing(null)}
              data-testid="file-viewer-back"
            >
              Back
            </button>
          </div>
          <pre>{viewing.content}</pre>
        </div>
      )}
    </div>
  );
}

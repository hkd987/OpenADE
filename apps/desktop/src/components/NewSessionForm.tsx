import { FormEvent, useEffect, useState } from "react";
import {
  createSession,
  HARNESSES,
  Harness,
  listProjects,
  SessionMeta,
} from "../api";

export function NewSessionForm({
  onCreated,
  onClose,
  initialRepo,
}: {
  onCreated: (session: SessionMeta) => void;
  onClose: () => void;
  /** Pre-filled repository (a project group's "+" button). */
  initialRepo?: string;
}) {
  const [title, setTitle] = useState("");
  const [harness, setHarness] = useState<Harness>("claude-code");
  const [repoRoot, setRepoRoot] = useState(initialRepo ?? "");
  const [entityRef, setEntityRef] = useState("");
  const [prompt, setPrompt] = useState("");
  const [projects, setProjects] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    listProjects()
      .then(({ projects }) => {
        setProjects(projects);
        setRepoRoot((current) => (current === "" ? (projects[0] ?? "") : current));
      })
      .catch(() => setProjects([]));
  }, []);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const session = await createSession({
        title,
        harness,
        repo_root: repoRoot,
        entity_ref: entityRef === "" ? undefined : entityRef,
        prompt: prompt === "" ? undefined : prompt,
      });
      onCreated(session);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="new-session" onSubmit={submit} data-testid="new-session-form">
      <div className="form-row">
        <label htmlFor="ns-title">Task</label>
        <input
          id="ns-title"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="e.g. add retries to the payments client"
          required
          data-testid="ns-title"
        />
      </div>
      <div className="form-row">
        <label htmlFor="ns-harness">Harness</label>
        <select
          id="ns-harness"
          value={harness}
          onChange={(e) => setHarness(e.target.value as Harness)}
          data-testid="ns-harness"
        >
          {HARNESSES.map((h) => (
            <option key={h.id} value={h.id}>
              {h.label} · {h.vendor}
            </option>
          ))}
        </select>
      </div>
      <div className="form-row">
        <label htmlFor="ns-repo">Repository</label>
        <input
          id="ns-repo"
          value={repoRoot}
          onChange={(e) => setRepoRoot(e.target.value)}
          placeholder="/path/to/repo"
          list="ns-projects"
          required
          data-testid="ns-repo"
        />
        <datalist id="ns-projects">
          {projects.map((p) => (
            <option key={p} value={p} />
          ))}
        </datalist>
      </div>
      <div className="form-row">
        <label htmlFor="ns-entity">Memory entity (optional)</label>
        <input
          id="ns-entity"
          value={entityRef}
          onChange={(e) => setEntityRef(e.target.value)}
          placeholder="component:default/payments-api or repo:acme/payments"
          data-testid="ns-entity"
        />
        <span className="form-hint" data-testid="ns-entity-hint">
          Blank = auto-detected from the repo&rsquo;s GitHub remote ·
          Backstage: <code>component:ns/name</code> · GitHub (via{" "}
          <code>gh</code>): <code>repo:owner/name</code>
        </span>
      </div>
      <div className="form-row">
        <label htmlFor="ns-prompt">Initial prompt (optional)</label>
        <textarea
          id="ns-prompt"
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          rows={3}
          data-testid="ns-prompt"
        />
      </div>
      {error !== null && (
        <div className="form-error" role="alert">
          {error}
        </div>
      )}
      <div className="form-actions">
        <button type="submit" disabled={busy} data-testid="ns-submit">
          {busy ? "Launching…" : "Launch session"}
        </button>
        <button type="button" onClick={onClose} className="secondary">
          Cancel
        </button>
      </div>
    </form>
  );
}

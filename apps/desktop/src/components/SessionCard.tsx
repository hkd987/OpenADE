import { SessionMeta } from "../api";

const HARNESS_LABELS: Record<SessionMeta["harness"], string> = {
  "claude-code": "Claude Code",
  "codex-cli": "Codex CLI",
  "gemini-cli": "Gemini CLI",
};

export function SessionCard({
  session,
  selected,
  onSelect,
}: {
  session: SessionMeta;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      className={`session-card ${selected ? "selected" : ""}`}
      onClick={onSelect}
      type="button"
    >
      <div className="session-card-top">
        <span className={`state state-${session.state}`}>{session.state}</span>
        <span className="harness">{HARNESS_LABELS[session.harness]}</span>
      </div>
      <div className="session-title">{session.title}</div>
      {session.entity_ref && (
        <div className="entity-ref" title="Catalog entity">
          {session.entity_ref}
        </div>
      )}
      {session.branch && <div className="branch">{session.branch}</div>}
    </button>
  );
}

import { harnessLabel, SessionMeta, splitEntityRef, timeAgo } from "../api";

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
        <span className="harness">{harnessLabel(session.harness)}</span>
      </div>
      <div className="session-title">{session.title}</div>
      {session.entity_ref && (
        <div className="entity-ref" title="Memory entity">
          <span
            className={`entity-chip entity-chip-${splitEntityRef(session.entity_ref).kind}`}
            data-testid="entity-chip"
          >
            <span className="entity-kind">
              {splitEntityRef(session.entity_ref).kind}
            </span>
            {splitEntityRef(session.entity_ref).rest}
          </span>
        </div>
      )}
      {session.branch && <div className="branch">{session.branch}</div>}
      <div className="session-age">{timeAgo(session.created_at)}</div>
    </button>
  );
}

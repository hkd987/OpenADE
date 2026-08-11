import { useState } from "react";
import { projectName, SessionMeta } from "../api";

/**
 * ⌘K command palette: jump to any session by typing, or launch a new one.
 * Kept deliberately small — the two actions an operator reaches for
 * mid-flow are "switch session" and "new session".
 */
export function CommandPalette({
  sessions,
  onSelect,
  onNewSession,
  onClose,
}: {
  sessions: SessionMeta[];
  onSelect: (id: string) => void;
  onNewSession: () => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);

  const q = query.toLowerCase();
  const matches = sessions.filter((s) =>
    `${s.title} ${projectName(s.repo_root)} ${s.harness}`
      .toLowerCase()
      .includes(q),
  );
  // "New session" is always reachable — first when idle, last when typing.
  const items: ({ kind: "new" } | { kind: "session"; session: SessionMeta })[] =
    q === ""
      ? [
          { kind: "new" },
          ...matches.map((s) => ({ kind: "session" as const, session: s })),
        ]
      : [
          ...matches.map((s) => ({ kind: "session" as const, session: s })),
          { kind: "new" },
        ];
  const activeIdx = Math.min(active, items.length - 1);

  const activate = (item: (typeof items)[number]) => {
    if (item.kind === "new") {
      onNewSession();
    } else {
      onSelect(item.session.id);
    }
    onClose();
  };

  return (
    <div
      className="palette-overlay"
      data-testid="palette"
      onClick={onClose}
      role="presentation"
    >
      <div
        className="palette-card"
        onClick={(e) => e.stopPropagation()}
        role="presentation"
      >
        <input
          autoFocus
          value={query}
          placeholder="Jump to a session, or start a new one…"
          data-testid="palette-input"
          onChange={(e) => {
            setQuery(e.target.value);
            setActive(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              onClose();
            } else if (e.key === "ArrowDown") {
              e.preventDefault();
              setActive((i) => Math.min(i + 1, items.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setActive((i) => Math.max(i - 1, 0));
            } else if (e.key === "Enter") {
              activate(items[activeIdx]);
            }
          }}
        />
        <ul className="palette-list">
          {items.map((item, idx) => (
            <li key={item.kind === "new" ? "new" : item.session.id}>
              <button
                type="button"
                className={`palette-item ${idx === activeIdx ? "active" : ""}`}
                data-testid="palette-item"
                onClick={() => activate(item)}
              >
                {item.kind === "new" ? (
                  <span className="palette-new">＋ New session</span>
                ) : (
                  <>
                    <span className="palette-title">{item.session.title}</span>
                    <span className="palette-meta">
                      {projectName(item.session.repo_root)} ·{" "}
                      {item.session.state}
                    </span>
                  </>
                )}
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

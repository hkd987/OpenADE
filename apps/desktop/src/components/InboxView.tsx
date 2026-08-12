import { useCallback, useEffect, useState } from "react";
import {
  DISMISS_REASONS,
  DismissReason,
  dismissInboxItem,
  getInboxItem,
  Harness,
  HARNESSES,
  InboxItem,
  InboxItemDetail,
  listInbox,
  SessionMeta,
  startFromInbox,
  timeAgo,
} from "../api";

/**
 * The Inbox: signals ingested through `POST /signals` land here for the
 * team (workspace server) or just you (embedded local inbox) to triage.
 * One-screen decisions, newest first: accept → a triage session launches
 * with the evidence; dismiss → a structured reason lands in outcome
 * memory and steers future triage. Items someone accepted move to
 * "In progress" with their name — nobody duplicates work.
 *
 * Keyboard: j/k move, o opens, a accepts (and starts a session with the
 * defaults), d opens the dismiss dialog, 1–4 pick the reason.
 */
export function InboxView({
  repos,
  onLaunched,
}: {
  /** Known local repositories, to prefill the session target. */
  repos: string[];
  onLaunched: (session: SessionMeta) => void;
}) {
  const [items, setItems] = useState<InboxItem[]>([]);
  const [detail, setDetail] = useState<InboxItemDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [focus, setFocus] = useState(0);
  const [dismissing, setDismissing] = useState<number | null>(null);
  const [harness, setHarness] = useState<Harness>("claude-code");
  const [repo, setRepo] = useState(repos[0] ?? "");
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const { items } = await listInbox();
      setItems(items);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const open = useCallback((id: number) => {
    getInboxItem(id)
      .then(setDetail)
      .catch((err) =>
        setError(err instanceof Error ? err.message : String(err)),
      );
  }, []);

  const launch = useCallback(
    async (itemId: number, investigate: boolean) => {
      if (repo === "") {
        setError("pick a repository to run the triage session in");
        return;
      }
      setBusy(true);
      setError(null);
      try {
        const meta = await startFromInbox({
          item_id: itemId,
          harness,
          repo_root: repo,
          investigate,
        });
        onLaunched(meta);
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setBusy(false);
      }
    },
    [harness, repo, onLaunched],
  );

  const dismiss = useCallback(
    async (itemId: number, reason: DismissReason) => {
      setBusy(true);
      setError(null);
      try {
        await dismissInboxItem(itemId, reason);
        setDismissing(null);
        setDetail(null);
        await refresh();
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setBusy(false);
      }
    },
    [refresh],
  );

  const fresh = items.filter((i) => i.status === "new");
  const inProgress = items.filter((i) => i.status === "accepted");
  const dismissed = items.filter((i) => i.status === "dismissed");

  // Keyboard triage. Scoped to this view (unbinds on unmount); modifier
  // combos (⌘K palette) and typing into fields pass through untouched.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) {
        return;
      }
      const target = e.target as HTMLElement | null;
      if (
        target instanceof HTMLInputElement ||
        target instanceof HTMLSelectElement ||
        target instanceof HTMLTextAreaElement
      ) {
        return;
      }
      if (dismissing !== null) {
        const reason = DISMISS_REASONS.find((r) => r.kbd === e.key);
        if (reason !== undefined) {
          void dismiss(dismissing, reason.id);
        }
        if (e.key === "Escape") {
          setDismissing(null);
        }
        return;
      }
      const focused = fresh[focus];
      switch (e.key) {
        case "j":
          setFocus((f) => Math.min(f + 1, fresh.length - 1));
          break;
        case "k":
          setFocus((f) => Math.max(f - 1, 0));
          break;
        case "o":
          if (focused !== undefined) {
            open(focused.id);
          }
          break;
        case "a":
          if (focused !== undefined) {
            void launch(focused.id, false);
          }
          break;
        case "d":
          if (focused !== undefined) {
            setDismissing(focused.id);
          }
          break;
        case "Escape":
          setDetail(null);
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [fresh, focus, dismissing, open, launch, dismiss]);

  const row = (item: InboxItem, index?: number) => (
    <button
      key={item.id}
      type="button"
      className={`inbox-row ${index !== undefined && index === focus ? "focused" : ""}`}
      data-testid="inbox-row"
      onClick={() => open(item.id)}
    >
      <div className="inbox-row-head">
        <span className={`severity ${item.severity}`}>{item.severity}</span>
        <strong>{item.title}</strong>
        {item.affected_count != null && (
          <span className="inbox-affected">affected {item.affected_count}</span>
        )}
        {timeAgo(item.last_seen) !== "" && (
          <span className="inbox-age">{timeAgo(item.last_seen)}</span>
        )}
      </div>
      {item.status === "accepted" && (
        <div className="inbox-row-meta" data-testid="inbox-taken">
          Accepted by <strong>{item.decided_by}</strong>
          {item.decided_at !== undefined &&
            timeAgo(item.decided_at) !== "" &&
            ` · ${timeAgo(item.decided_at)}`}
        </div>
      )}
      {item.status === "dismissed" && (
        <div className="inbox-row-meta" data-testid="inbox-dismissed">
          Dismissed ({item.dismiss_reason}) by{" "}
          <strong>{item.decided_by}</strong>
        </div>
      )}
    </button>
  );

  return (
    <main className="inbox-view" data-testid="inbox-view">
      {error !== null && (
        <div className="form-error" role="alert" data-testid="inbox-error">
          {error}
        </div>
      )}

      {dismissing !== null && (
        <div className="dismiss-overlay" data-testid="dismiss-dialog">
          <div className="dismiss-card">
            <h3>Dismiss this item?</h3>
            <p className="dismiss-hint">
              The reason is recorded in outcome memory and steers future
              triage — “intended behavior” reroutes recurrences away from
              code changes.
            </p>
            {DISMISS_REASONS.map((r) => (
              <button
                key={r.id}
                type="button"
                className="dismiss-reason"
                data-testid={`dismiss-${r.id}`}
                disabled={busy}
                onClick={() => void dismiss(dismissing, r.id)}
              >
                <kbd>{r.kbd}</kbd> {r.label}
              </button>
            ))}
            <button
              type="button"
              className="secondary"
              data-testid="dismiss-cancel"
              onClick={() => setDismissing(null)}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {detail === null ? (
        <section className="inbox-list" aria-label="Inbox">
          {items.length === 0 && error === null && (
            <p className="empty" data-testid="inbox-empty">
              No signals yet — point your tools at <code>POST /signals</code>{" "}
              (see docs/signals.md) and triage lands here.
            </p>
          )}
          {fresh.length > 0 && <h4 className="inbox-group">New</h4>}
          {fresh.map((item, i) => row(item, i))}
          {inProgress.length > 0 && (
            <h4 className="inbox-group">In progress</h4>
          )}
          {inProgress.map((item) => row(item))}
          {dismissed.length > 0 && <h4 className="inbox-group">Dismissed</h4>}
          {dismissed.map((item) => row(item))}
        </section>
      ) : (
        <section className="inbox-detail" data-testid="inbox-detail">
          <div className="file-viewer-bar">
            <span className={`severity ${detail.item.severity}`}>
              {detail.item.severity}
            </span>
            <strong>{detail.item.title}</strong>
            <button
              type="button"
              className="secondary"
              onClick={() => setDetail(null)}
              data-testid="inbox-back"
            >
              Back
            </button>
          </div>

          {detail.item.status === "new" ? (
            <div className="inbox-actions">
              <select
                value={harness}
                onChange={(e) => setHarness(e.target.value as Harness)}
                title="Harness for the triage session"
                data-testid="triage-harness"
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
                title="Repository to triage in"
                data-testid="triage-repo"
              />
              <button
                disabled={busy}
                onClick={() => void launch(detail.item.id, false)}
                data-testid="accept-button"
              >
                Accept & start session
              </button>
              <button
                disabled={busy}
                className="secondary"
                onClick={() => void launch(detail.item.id, true)}
                data-testid="investigate-button"
              >
                Investigate with agent
              </button>
              <button
                disabled={busy}
                className="danger"
                onClick={() => setDismissing(detail.item.id)}
                data-testid="dismiss-button"
              >
                Dismiss…
              </button>
            </div>
          ) : (
            <div className="inbox-row-meta" data-testid="inbox-decided">
              {detail.item.status === "accepted" ? "Accepted" : "Dismissed"} by{" "}
              <strong>{detail.item.decided_by}</strong>
              {detail.item.dismiss_reason !== undefined &&
                ` (${detail.item.dismiss_reason})`}
            </div>
          )}

          {detail.signals.map((sig) => (
            <div key={sig.id} className="inbox-signal" data-testid="inbox-signal">
              <div className="inbox-row-meta">
                from <strong>{sig.source}</strong> · {sig.kind}
              </div>
              {sig.body !== "" && <p className="inbox-body">{sig.body}</p>}
              {sig.evidence.length > 0 && (
                <div className="inbox-evidence">
                  {sig.evidence.map((e) => (
                    <a
                      key={e.url}
                      href={e.url}
                      target="_blank"
                      rel="noreferrer"
                      className="evidence-chip"
                    >
                      {e.label} ↗
                    </a>
                  ))}
                </div>
              )}
            </div>
          ))}

          {detail.outcomes.length > 0 && (
            <>
              <h4 className="inbox-group">Prior outcomes on this signal</h4>
              <ul className="inbox-outcomes" data-testid="inbox-outcomes">
                {detail.outcomes.map((o, i) => (
                  <li key={i}>
                    <code>{o.kind}</code> on {o.occurred_at.split("T")[0]}
                    {o.note !== undefined && ` (${o.note})`}
                    {o.pr_url !== undefined && (
                      <>
                        {" "}
                        <a href={o.pr_url} target="_blank" rel="noreferrer">
                          PR ↗
                        </a>
                      </>
                    )}
                  </li>
                ))}
              </ul>
            </>
          )}
        </section>
      )}
    </main>
  );
}

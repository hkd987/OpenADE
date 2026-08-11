import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { getScrollback, sendInput, SessionMeta } from "../api";

const POLL_MS = 1000;

/**
 * Read-mostly terminal attached to a daemon session.
 *
 * v0 renders the scrollback snapshot and forwards keystrokes; a streaming
 * transport (SSE/WebSocket from the daemon) replaces polling in a later
 * milestone.
 */
export function TerminalView({ session }: { session: SessionMeta }) {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }
    const term = new Terminal({
      convertEol: true,
      fontSize: 13,
      scrollback: 5000,
    });
    term.open(container);

    let written = 0;
    let cancelled = false;

    const poll = async () => {
      try {
        const { scrollback } = await getScrollback(session.id);
        if (cancelled) {
          return;
        }
        if (scrollback.length < written) {
          // Daemon restarted or buffer trimmed: redraw from scratch.
          term.reset();
          written = 0;
        }
        if (scrollback.length > written) {
          term.write(scrollback.slice(written));
          written = scrollback.length;
        }
      } catch {
        // Daemon unreachable; App-level banner reports it.
      }
    };

    void poll();
    const timer = setInterval(() => void poll(), POLL_MS);
    const inputDisposable = term.onData((data) => {
      void sendInput(session.id, data).catch(() => undefined);
    });

    return () => {
      cancelled = true;
      clearInterval(timer);
      inputDisposable.dispose();
      term.dispose();
    };
  }, [session.id]);

  return (
    <div className="terminal-view" data-testid="terminal-view">
      <div className="terminal-container" ref={containerRef} />
    </div>
  );
}

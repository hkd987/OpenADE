import { Plus, TerminalWindow, X } from "@phosphor-icons/react";
import { useCallback, useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import {
  createTerminal,
  listTerminals,
  ProjectTerminal,
  resizeProjectTerminal,
  sendTerminalInput,
  stopTerminal,
  terminalStreamURL,
} from "./api";

export function TerminalWorkspace({ sessionId }: { sessionId: string }) {
  const [terminals, setTerminals] = useState<ProjectTerminal[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [hidden, setHidden] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const next = await listTerminals(sessionId);
    setTerminals(next);
    setActiveId((current) => current ?? next.find((item) => item.status === "running")?.id ?? null);
  }, [sessionId]);

  useEffect(() => {
    setHidden(new Set());
    void refresh().catch((reason) => setError(String(reason)));
  }, [refresh]);

  const openTerminal = async () => {
    setBusy(true);
    setError(null);
    try {
      const terminal = await createTerminal(sessionId);
      setTerminals((current) => [...current, terminal]);
      setActiveId(terminal.id);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const closeTerminal = async (terminal: ProjectTerminal) => {
    if (terminal.status === "running") await stopTerminal(terminal.id).catch(() => undefined);
    setHidden((current) => new Set(current).add(terminal.id));
    setActiveId((current) => current === terminal.id ? null : current);
  };

  const visible = terminals.filter((terminal) => !hidden.has(terminal.id));
  const active = visible.find((terminal) => terminal.id === activeId) ?? visible.at(-1) ?? null;

  return (
    <section className="terminal-workspace">
      <header className="terminal-tabs">
        {visible.map((terminal) => (
          <button
            type="button"
            className={active?.id === terminal.id ? "active" : ""}
            key={terminal.id}
            onClick={() => setActiveId(terminal.id)}
          >
            <TerminalWindow /><span>{terminal.title}</span>
            <i className={`terminal-state ${terminal.status}`} />
            <span
              className="terminal-close"
              role="button"
              tabIndex={0}
              aria-label={`Close ${terminal.title}`}
              onClick={(event) => { event.stopPropagation(); void closeTerminal(terminal); }}
              onKeyDown={(event) => { if (event.key === "Enter") void closeTerminal(terminal); }}
            ><X /></span>
          </button>
        ))}
        <button className="terminal-new" type="button" onClick={() => void openTerminal()} disabled={busy} aria-label="New terminal"><Plus /></button>
      </header>
      {error && <div className="inline-error">{error}</div>}
      {active ? <TerminalHost key={active.id} terminal={active} /> : (
        <div className="terminal-empty">
          <span><TerminalWindow /></span>
          <strong>Project terminal</strong>
          <p>Open an independent shell in this session’s isolated worktree.</p>
          <button type="button" onClick={() => void openTerminal()} disabled={busy}><Plus /> New terminal</button>
        </div>
      )}
    </section>
  );
}

function TerminalHost({ terminal: projectTerminal }: { terminal: ProjectTerminal }) {
  const hostRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const terminal = new Terminal({
      cursorBlink: projectTerminal.status === "running",
      cursorStyle: "bar",
      disableStdin: projectTerminal.status !== "running",
      fontFamily: '"SFMono-Regular", "JetBrains Mono", Menlo, monospace',
      fontSize: 12.5,
      lineHeight: 1.35,
      scrollback: 10_000,
      theme: {
        background: "#111315", foreground: "#d9dddf", cursor: "#f2f4f5",
        selectionBackground: "#38566f88", black: "#111315", red: "#f06a6a",
        green: "#75d196", yellow: "#e3b764", blue: "#69aee8",
        magenta: "#c59bea", cyan: "#76c7c2", white: "#e9ecee", brightBlack: "#687078",
      },
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(host);

    let disposed = false;
    const socket = new WebSocket(terminalStreamURL(projectTerminal.id));
    socket.onmessage = (event) => {
      const message = JSON.parse(String(event.data)) as { type: string; data?: string };
      if (message.type === "output" && message.data) terminal.write(message.data);
    };
    const fitAndResize = () => {
      if (disposed || host.clientWidth === 0 || host.clientHeight === 0) return;
      fit.fit();
      if (projectTerminal.status === "running") {
        void resizeProjectTerminal(projectTerminal.id, terminal.rows, terminal.cols).catch(() => undefined);
      }
    };
    const observer = new ResizeObserver(fitAndResize);
    observer.observe(host);
    requestAnimationFrame(fitAndResize);
    const input = terminal.onData((data) => {
      void sendTerminalInput(projectTerminal.id, data).catch(() => undefined);
    });

    return () => {
      disposed = true;
      observer.disconnect();
      input.dispose();
      socket.close();
      terminal.dispose();
    };
  }, [projectTerminal.id, projectTerminal.status]);

  return <div className="terminal-host" ref={hostRef} aria-label={`${projectTerminal.title} in project worktree`} />;
}

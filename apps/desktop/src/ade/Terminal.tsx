import { Cpu, Play, Plus, TerminalWindow, X } from "@phosphor-icons/react";
import { useCallback, useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import {
  createTerminal,
  listTerminals,
  ProjectTerminal,
  resizeProjectTerminal,
  resizeTerminal,
  resumeTUI,
  sendInput,
  sendTerminalInput,
  Session,
  stopTerminal,
  streamURL,
  terminalStreamURL,
} from "./api";

export function TerminalWorkspace({ session }: { session: Session }) {
  const sessionId = session.id;
  const [terminals, setTerminals] = useState<ProjectTerminal[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [hidden, setHidden] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(false);

  useEffect(() => {
    mountedRef.current = true;
    return () => { mountedRef.current = false; };
  }, []);

  const refresh = useCallback(async () => {
    const next = await listTerminals(sessionId);
    if (!mountedRef.current) return;
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
      if (!mountedRef.current) return;
      setTerminals((current) => [...current, terminal]);
      setActiveId(terminal.id);
    } catch (reason) {
      if (mountedRef.current) setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  };

  const openAgentTUI = async () => {
    setBusy(true);
    setError(null);
    try {
      const terminal = await createTerminal(sessionId, { kind: "agent", agent: session.agent, resume: true });
      if (!mountedRef.current) return;
      setTerminals((current) => [...current, terminal]);
      setActiveId(terminal.id);
    } catch (reason) {
      if (mountedRef.current) setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  };

  const closeTerminal = async (terminal: ProjectTerminal) => {
    if (terminal.status === "running") await stopTerminal(terminal.id).catch(() => undefined);
    if (!mountedRef.current) return;
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
        {["codex", "codex-cli", "claude", "claude-code"].includes(session.agent) && <button className="terminal-new agent-tui-new" type="button" onClick={() => void openAgentTUI()} disabled={busy} aria-label={`Open ${session.agent} TUI`}><Cpu /></button>}
      </header>
      {error && <div className="inline-error">{error}</div>}
      {active ? <TerminalHost key={active.id} terminal={active} /> : (
        <div className="terminal-empty">
          <span><TerminalWindow /></span>
          <strong>Project terminal</strong>
          <p>Open an independent shell in this session’s isolated worktree.</p>
          <div className="terminal-empty-actions"><button type="button" onClick={() => void openTerminal()} disabled={busy}><Plus /> New terminal</button>{["codex", "codex-cli", "claude", "claude-code"].includes(session.agent) && <button type="button" className="primary" onClick={() => void openAgentTUI()} disabled={busy}><Cpu /> Open {agentName(session.agent)} TUI</button>}</div>
        </div>
      )}
    </section>
  );
}

export function DirectTUIWorkspace({ session, onRefresh }: { session: Session; onRefresh: () => Promise<void> }) {
  const active = ["starting", "running", "waiting"].includes(session.status);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(false);
  useEffect(() => {
    mountedRef.current = true;
    return () => { mountedRef.current = false; };
  }, []);
  const resume = async () => {
    setBusy(true);
    setError(null);
    try {
      await resumeTUI(session.id);
      await onRefresh();
    } catch (reason) {
      if (mountedRef.current) setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  };
  return <section className={`direct-tui-workspace ${error ? "with-error" : ""}`}>
    <header><span><Cpu /> Direct {agentName(session.agent)} TUI</span><small>Daemon-hosted · {session.worktree_path}</small>{!active && <button type="button" onClick={() => void resume()} disabled={busy}><Play weight="fill" /> Resume</button>}</header>
    {error && <div className="inline-error">{error}</div>}
    <SessionTerminalHost session={session} />
  </section>;
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

    const socket = new WebSocket(terminalStreamURL(projectTerminal.id));
    socket.onmessage = (event) => {
      const message = JSON.parse(String(event.data)) as { type: string; data?: string };
      if (message.type === "output" && message.data) terminal.write(message.data);
    };
    const stopSizing = observeTerminalSize(host, terminal, fit, (rows, cols) => {
      if (projectTerminal.status === "running") {
        return resizeProjectTerminal(projectTerminal.id, rows, cols);
      }
      return Promise.resolve();
    });
    const input = terminal.onData((data) => {
      void sendTerminalInput(projectTerminal.id, data).catch(() => undefined);
    });

    return () => {
      socket.onmessage = null;
      socket.close();
      stopSizing();
      input.dispose();
      terminal.dispose();
    };
  }, [projectTerminal.id, projectTerminal.status]);

  return <div className="terminal-host" ref={hostRef} aria-label={`${projectTerminal.title} in project worktree`} />;
}

function SessionTerminalHost({ session }: { session: Session }) {
  const active = ["starting", "running", "waiting"].includes(session.status);
  const input = useCallback((data: string) => sendInput(session.id, data), [session.id]);
  const resize = useCallback((rows: number, cols: number) => resizeTerminal(session.id, rows, cols), [session.id]);
  return <TerminalSurface
    id={session.id}
    title={`${agentName(session.agent)} TUI`}
    running={active}
    socketURL={streamURL(session.id)}
    onInput={input}
    onResize={resize}
  />;
}

function TerminalSurface({ id, title, running, socketURL, onInput, onResize }: { id: string; title: string; running: boolean; socketURL: string; onInput: (data: string) => Promise<void>; onResize: (rows: number, cols: number) => Promise<void> }) {
  const hostRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const terminal = new Terminal({
      cursorBlink: running, cursorStyle: "bar", disableStdin: !running,
      fontFamily: '"SFMono-Regular", "JetBrains Mono", Menlo, monospace', fontSize: 12.5, lineHeight: 1.35, scrollback: 10_000,
      theme: { background: "#111315", foreground: "#d9dddf", cursor: "#f2f4f5", selectionBackground: "#38566f88", black: "#111315", red: "#f06a6a", green: "#75d196", yellow: "#e3b764", blue: "#69aee8", magenta: "#c59bea", cyan: "#76c7c2", white: "#e9ecee", brightBlack: "#687078" },
    });
    const fit = new FitAddon(); terminal.loadAddon(fit); terminal.open(host);
    const socket = new WebSocket(socketURL);
    socket.onmessage = (event) => { const message = JSON.parse(String(event.data)) as { type: string; data?: string }; if (message.type === "output" && message.data) terminal.write(message.data); };
    const stopSizing = observeTerminalSize(host, terminal, fit, (rows, cols) => running ? onResize(rows, cols) : Promise.resolve());
    const input = terminal.onData((data) => { void onInput(data).catch(() => undefined); });
    return () => {
      socket.onmessage = null;
      socket.close();
      stopSizing();
      input.dispose();
      terminal.dispose();
    };
  }, [id, onInput, onResize, running, socketURL]);
  return <div className="terminal-host direct-tui-host" ref={hostRef} aria-label={`${title} in project worktree`} />;
}

function observeTerminalSize(
  host: HTMLDivElement,
  terminal: Terminal,
  fit: FitAddon,
  resizePTY: (rows: number, cols: number) => Promise<void>,
) {
  let disposed = false;
  let frame: number | null = null;
  let lastSize = "";

  const fitAndResize = () => {
    frame = null;
    if (disposed || host.clientWidth === 0 || host.clientHeight === 0) return;
    fit.fit();
    const nextSize = `${terminal.rows}x${terminal.cols}`;
    if (nextSize === lastSize) return;
    lastSize = nextSize;
    void resizePTY(terminal.rows, terminal.cols).catch(() => undefined);
  };
  const scheduleFit = () => {
    if (disposed) return;
    if (frame !== null) cancelAnimationFrame(frame);
    frame = requestAnimationFrame(fitAndResize);
  };

  const observer = new ResizeObserver(scheduleFit);
  observer.observe(host);
  window.addEventListener("resize", scheduleFit);
  scheduleFit();
  void document.fonts?.ready.then(scheduleFit);

  return () => {
    disposed = true;
    observer.disconnect();
    window.removeEventListener("resize", scheduleFit);
    if (frame !== null) cancelAnimationFrame(frame);
  };
}

function agentName(agent: string): string {
  return agent.includes("claude") ? "Claude Code" : "Codex";
}

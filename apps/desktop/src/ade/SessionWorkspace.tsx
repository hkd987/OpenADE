import {
  ArrowLeft,
  ArrowUp,
  DotsThree,
  GitBranch,
  GitDiff,
  GithubLogo,
  Plus,
  SpinnerGap,
  Square,
  TerminalWindow,
  Ticket as TicketIcon,
  X,
} from "@phosphor-icons/react";
import { FormEvent, ReactNode, useCallback, useEffect, useRef, useState } from "react";
import {
  createPullRequest,
  enqueueMessage,
  getTicket,
  listAgentCommands,
  listMessageQueue,
  projectName,
  QueuedMessage,
  removeQueuedMessage as removeQueuedMessageRequest,
  Session,
  steerQueuedMessage as steerQueuedMessageRequest,
  stopSession,
  streamURL,
  Ticket,
  AgentCommand,
} from "./api";
import { AgentCommandMenu, filterAgentCommands } from "./AgentCommandMenu";
import { ChatTimeline } from "./ChatTimeline";
import { ReviewWorkspace } from "./ReviewWorkspace";
import { DirectTUIWorkspace, TerminalWorkspace } from "./Terminal";
import { Preferences } from "./preferences";
import { MessageQueue } from "./MessageQueue";

type WorkTab = "review" | "terminal" | "pull-request" | "ticket";

export function SessionWorkspace({ session, preferences, onBack, onRefresh }: { session: Session; preferences: Preferences; onBack: () => void; onRefresh: () => Promise<void> }) {
  const tuiMode = session.mode === "tui";
  const defaultTab: WorkTab = session.agent === "shell" || (preferences.session_surface === "terminal" && !tuiMode) ? "terminal" : "review";
  const [tab, setTab] = useState<WorkTab>(defaultTab);
  const [rightOpen, setRightOpen] = useState(session.agent === "shell" || (preferences.session_surface === "terminal" && !tuiMode));
  const [ticket, setTicket] = useState<Ticket | null>(null);
  const [panelError, setPanelError] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [output, setOutput] = useState("");
  const [busy, setBusy] = useState(false);
  const [streamVersion, setStreamVersion] = useState(0);
  const [commands, setCommands] = useState<AgentCommand[]>([]);
  const [commandOpen, setCommandOpen] = useState(false);
  const [queuedMessages, setQueuedMessages] = useState<QueuedMessage[]>([]);
  const outputRef = useRef<HTMLDivElement>(null);
  const reconnectStreamRef = useRef(false);
  const mountedRef = useRef(false);
  const focusTimerRef = useRef<number | undefined>(undefined);
  const active = ["running", "starting", "waiting"].includes(session.status);
  const resumable = ["claude", "claude-code", "codex", "codex-cli"].includes(session.agent) && !active;
  const chatCapable = session.agent !== "shell" && !tuiMode;
  const canMessage = chatCapable && (active || resumable);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      if (focusTimerRef.current !== undefined) window.clearTimeout(focusTimerRef.current);
    };
  }, []);

  useEffect(() => {
    setTab(defaultTab);
    setRightOpen(session.agent === "shell" || (preferences.session_surface === "terminal" && !tuiMode));
    setPanelError(null);
    setTicket(null);
  }, [defaultTab, preferences.session_surface, session.agent, session.id, tuiMode]);

  const refreshQueue = useCallback(async () => {
    if (!chatCapable) return;
    try {
      const next = await listMessageQueue(session.id);
      if (mountedRef.current) setQueuedMessages(next);
    } catch {
      // The session refresh loop will surface daemon connectivity errors globally.
    }
  }, [chatCapable, session.id]);

  useEffect(() => {
    if (!chatCapable) return;
    let stopped = false;
    let timer: number | undefined;
    const poll = async () => {
      await refreshQueue();
      if (!stopped) timer = window.setTimeout(() => void poll(), 800);
    };
    void poll();
    return () => {
      stopped = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [chatCapable, refreshQueue]);

  useEffect(() => {
    reconnectStreamRef.current = active || queuedMessages.length > 0;
  }, [active, queuedMessages.length]);

  useEffect(() => {
    if (tuiMode || !chatCapable) return;
    let disposed = false;
    let reconnectTimer: number | undefined;
    const socket = new WebSocket(streamURL(session.id));
    socket.onmessage = (event) => {
      if (disposed) return;
      const message = JSON.parse(String(event.data)) as { type: string; data?: string };
      if (message.type === "output" && message.data) {
        setOutput((current) => (current + message.data!).slice(-2_000_000));
      }
    };
    socket.onclose = () => {
      if (!disposed && reconnectStreamRef.current) {
        reconnectTimer = window.setTimeout(() => {
          if (!disposed) setStreamVersion((current) => current + 1);
        }, 250);
      }
    };
    return () => {
      disposed = true;
      if (reconnectTimer !== undefined) window.clearTimeout(reconnectTimer);
      socket.onmessage = null;
      socket.onclose = null;
      socket.close();
    };
  }, [chatCapable, session.id, streamVersion, tuiMode]);

  useEffect(() => setOutput(""), [session.id]);

  useEffect(() => {
    if (!chatCapable) return;
    let stale = false;
    void listAgentCommands(session.id)
      .then((next) => { if (!stale) setCommands(next); })
      .catch(() => { if (!stale) setCommands([]); });
    return () => { stale = true; };
  }, [chatCapable, session.id]);

  useEffect(() => {
    outputRef.current?.scrollTo({ top: outputRef.current.scrollHeight, behavior: "smooth" });
  }, [output]);

  useEffect(() => {
    let stale = false;
    if (tab === "ticket" && session.ticket_key && !ticket) {
      void getTicket(session.ticket_key)
        .then((next) => { if (!stale) setTicket(next); })
        .catch((reason) => { if (!stale) setPanelError(String(reason)); });
    }
    return () => { stale = true; };
  }, [session.ticket_key, tab, ticket]);

  const focusComposer = () => {
    if (focusTimerRef.current !== undefined) window.clearTimeout(focusTimerRef.current);
    focusTimerRef.current = window.setTimeout(() => {
      focusTimerRef.current = undefined;
      document.querySelector<HTMLTextAreaElement>(".session-composer textarea")?.focus();
    }, 0);
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!input.trim() || !canMessage) return;
    const value = input.trim();
    setInput("");
    setPanelError(null);
    try {
      const queued = await enqueueMessage(session.id, value);
      setQueuedMessages((current) => [...current.filter((item) => item.id !== queued.id), queued]);
      await onRefresh();
      await refreshQueue();
    } catch (reason) {
      setInput(value);
      setPanelError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const steerQueuedMessage = async (id: string) => {
    setQueuedMessages((current) => {
      const selected = current.find((item) => item.id === id);
      return selected ? [selected, ...current.filter((item) => item.id !== id)] : current;
    });
    try {
      await steerQueuedMessageRequest(session.id, id);
      await refreshQueue();
    } catch (reason) {
      setPanelError(reason instanceof Error ? reason.message : String(reason));
      await refreshQueue();
    }
  };

  const removeQueuedMessage = async (id: string) => {
    setQueuedMessages((current) => current.filter((item) => item.id !== id));
    try {
      await removeQueuedMessageRequest(session.id, id);
    } catch (reason) {
      setPanelError(reason instanceof Error ? reason.message : String(reason));
      await refreshQueue();
    }
  };

  const editQueuedMessage = async (id: string) => {
    const selected = queuedMessages.find((item) => item.id === id);
    if (!selected) return;
    setQueuedMessages((current) => current.filter((item) => item.id !== id));
    setInput(selected.text);
    focusComposer();
    try {
      await removeQueuedMessageRequest(session.id, id);
    } catch (reason) {
      setPanelError(reason instanceof Error ? reason.message : String(reason));
      setInput("");
      await refreshQueue();
    }
  };

  const insertCommand = (command: AgentCommand) => {
    setInput(`${command.invocation} `);
    setCommandOpen(false);
    focusComposer();
  };

  const createPR = async () => {
    setBusy(true);
    setPanelError(null);
    try {
      await createPullRequest({
        sessionId: session.id,
        title: session.title,
        base: session.base_branch,
        body: `## Summary\n\n${session.prompt}\n\n${session.ticket_key ? `Ticket: ${session.ticket_key}` : ""}\n\nCreated from OpenADE.`,
      });
      await onRefresh();
    } catch (reason) {
      setPanelError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const toggleSurface = (surface: WorkTab) => {
    if (rightOpen && tab === surface) {
      setRightOpen(false);
      return;
    }
    setTab(surface);
    setRightOpen(true);
  };

  return (
    <div className={`session-workspace ${rightOpen ? "with-panel" : ""} ${rightOpen && tab === "review" ? "panel-review" : ""} ${session.agent === "shell" ? "shell-workspace" : ""}`}>
      <header className="session-header">
        <button className="icon-button" onClick={onBack} aria-label="Back"><ArrowLeft /></button>
        <span className={`status-dot ${session.status}`} />
        <div className="session-title"><h1>{session.title}</h1><p>{projectName(session.repo_root)} <span>·</span> <code title={session.branch}>{session.branch}</code></p></div>
        <StatusPill status={session.status} />
        {active && <button className="header-stop" onClick={() => void stopSession(session.id).then(onRefresh)}><Square weight="fill" /> Stop</button>}
        <button className="icon-button" aria-label="Session actions"><DotsThree /></button>
      </header>
      <section className={`conversation ${tuiMode ? "tui-conversation" : ""}`}>
        {tuiMode ? <DirectTUIWorkspace session={session} onRefresh={onRefresh} /> : <>
        <div className="messages" ref={outputRef}>
          {chatCapable ? <ChatTimeline session={session} output={output} activityExpanded={preferences.activity_detail === "expanded"} /> : <div className="shell-session-note"><TerminalWindow /><div><strong>Terminal run</strong><p>This run stays in the terminal so command output never gets mixed into chat.</p></div></div>}
        </div>
        {canMessage ? <div className={`session-composer-dock ${queuedMessages.length ? "with-queue" : ""}`}>
          <MessageQueue messages={queuedMessages} sendingId={queuedMessages.find((item) => item.status === "dispatching")?.id ?? null} onSteer={(id) => void steerQueuedMessage(id)} onRemove={(id) => void removeQueuedMessage(id)} onEdit={(id) => void editQueuedMessage(id)} />
          <form className="session-composer" onSubmit={submit}>
          {commandOpen && <AgentCommandMenu commands={commands} input={input} onSelect={insertCommand} />}
          <textarea
            value={input}
            onChange={(event) => { const value = event.target.value; setInput(value); if (/^\s*[/ $]/.test(value)) setCommandOpen(true); }}
            placeholder={canMessage ? `Message ${agentLabel(session.agent)}…` : "This run does not support follow-up messages"}
            rows={2}
            disabled={!canMessage}
            onKeyDown={(event) => {
              if (event.key === "Escape" && commandOpen) {
                event.preventDefault();
                setCommandOpen(false);
                return;
              }
              if (event.key === "Enter" && commandOpen && !event.shiftKey) {
                const first = filterAgentCommands(commands, input)[0];
                if (first) {
                  event.preventDefault();
                  insertCommand(first);
                  return;
                }
              }
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                event.currentTarget.form?.requestSubmit();
              }
            }}
          />
          <div className="composer-footer">
            <button type="button" className={`command-trigger ${commandOpen ? "active" : ""}`} onClick={() => setCommandOpen((value) => !value)} aria-label="Skills and commands" title="Skills and commands"><Plus /></button>
            <span className="runtime-chip"><span className={`status-dot ${session.status}`} />{active ? queuedMessages.length ? `${queuedMessages.length} queued · agent working` : `${agentLabel(session.agent)} is attached` : queuedMessages.some((item) => item.status === "dispatching") ? "Sending next message" : resumable ? "Conversation can continue" : `Run ${session.status}`}</span>
            <button className="send-button" disabled={!canMessage || !input.trim()} aria-label="Send message"><ArrowUp weight="bold" /></button>
          </div>
          </form>
        </div> : <div className="session-closed-state"><span className={`status-dot ${session.status}`} />{chatCapable ? `This ${agentLabel(session.agent)} run is ${session.status}` : "Use the Terminal panel to inspect this run"}</div>}</>}
      </section>
      {rightOpen && (
        <aside className="work-panel" aria-label={`${workTabLabel(tab)} panel`}>
          <header className="work-panel-header">
            <span>{workTabIcon(tab)}<strong>{workTabLabel(tab)}</strong></span>
            {chatCapable && <button className="icon-button" onClick={() => setRightOpen(false)} aria-label={`Close ${workTabLabel(tab)} panel`}><X /></button>}
          </header>
          <div className="panel-body">
            {panelError && <div className="inline-error">{panelError}</div>}
            {tab === "terminal" ? <TerminalWorkspace session={session} /> : tab === "review" ? <ReviewWorkspace sessionId={session.id} /> : tab === "ticket" ? <TicketPanel ticket={ticket} session={session} /> : <PRPanel session={session} busy={busy} onCreate={createPR} onTicket={() => setTab("ticket")} />}
          </div>
        </aside>
      )}
      <aside className="inspector-rail" aria-label="Session tools">
        <InspectorButton active={rightOpen && tab === "review"} onClick={() => toggleSurface("review")} icon={<GitDiff />} label="Changes" />
        <InspectorButton active={rightOpen && tab === "terminal"} onClick={() => toggleSurface("terminal")} icon={<TerminalWindow />} label="Terminal" />
        <InspectorButton active={rightOpen && tab === "pull-request"} onClick={() => toggleSurface("pull-request")} icon={<GithubLogo />} label="PR" />
      </aside>
    </div>
  );
}

function InspectorButton({ active, icon, label, onClick }: { active: boolean; icon: ReactNode; label: string; onClick: () => void }) {
  return <button className={active ? "active" : ""} onClick={onClick} aria-label={label} aria-pressed={active} title={label}>{icon}<span>{label}</span></button>;
}

function workTabLabel(tab: WorkTab): string {
  return ({ review: "Changes", terminal: "Terminal", "pull-request": "Pull request", ticket: "Ticket" } as Record<WorkTab, string>)[tab];
}

function workTabIcon(tab: WorkTab): ReactNode {
  if (tab === "terminal") return <TerminalWindow />;
  if (tab === "pull-request") return <GithubLogo />;
  if (tab === "ticket") return <TicketIcon />;
  return <GitDiff />;
}

function PRPanel({ session, busy, onCreate, onTicket }: { session: Session; busy: boolean; onCreate: () => void; onTicket: () => void }) {
  return <div className="pr-panel"><div className="panel-kicker"><GithubLogo /> GitHub delivery</div><h2>{session.pr_url ? "Draft pull request created" : "Prepare this branch for review"}</h2><p>OpenADE keeps the ticket key, branch, commit policy, and draft PR connected to this session.</p><dl><div><dt>Head</dt><dd><code>{session.branch}</code></dd></div><div><dt>Base</dt><dd><code>{session.base_branch}</code></dd></div>{session.ticket_key && <div><dt>Ticket</dt><dd><button className="link-button" onClick={onTicket}>{session.ticket_key}</button></dd></div>}</dl>{session.pr_url ? <button className="primary-wide" onClick={() => window.open(session.pr_url, "_blank")}><GithubLogo /> Open pull request</button> : <button className="primary-wide" disabled={busy} onClick={onCreate}>{busy ? <SpinnerGap className="spin" /> : <GitBranch />} Push branch and create draft PR</button>}<small>This action uses your locally authenticated GitHub CLI.</small></div>;
}

function TicketPanel({ ticket, session }: { ticket: Ticket | null; session: Session }) {
  return <div className="ticket-panel"><div className="panel-kicker"><TicketIcon /> Linked work item</div><h2>{ticket?.summary || session.ticket_key}</h2><p>{ticket ? `${ticket.status || "Status unavailable"} · ${ticket.assignee || "Unassigned"}` : "Ticket details are linked to this session. Configure the Jira CLI to load live metadata."}</p>{session.ticket_url && <button className="primary-wide" onClick={() => window.open(session.ticket_url, "_blank")}><TicketIcon /> Open in Jira</button>}<dl><div><dt>Required branch prefix</dt><dd><code>{session.ticket_key?.toLowerCase()}/</code></dd></div><div><dt>Current branch</dt><dd><code>{session.branch}</code></dd></div></dl></div>;
}

function StatusPill({ status }: { status: string }) {
  return <span className={`status-pill ${status}`}><span className="status-dot" />{status.replace("-", " ")}</span>;
}

function agentLabel(agent: string): string {
  return ({ claude: "Claude Code", codex: "Codex CLI", copilot: "Copilot", opencode: "OpenCode" } as Record<string, string>)[agent] ?? agent;
}

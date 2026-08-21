import {
  ArrowLeft,
  ArrowUp,
  DotsThree,
  GitBranch,
  GitDiff,
  GithubLogo,
  SidebarSimple,
  SpinnerGap,
  Square,
  TerminalWindow,
  Ticket as TicketIcon,
  X,
} from "@phosphor-icons/react";
import { FormEvent, ReactNode, useEffect, useRef, useState } from "react";
import {
  createPullRequest,
  getTicket,
  projectName,
  sendInput,
  sendMessage,
  Session,
  stopSession,
  streamURL,
  Ticket,
} from "./api";
import { ChatTimeline } from "./ChatTimeline";
import { ReviewWorkspace } from "./ReviewWorkspace";
import { TerminalWorkspace } from "./Terminal";
import { Preferences } from "./preferences";

type WorkTab = "review" | "terminal" | "pull-request" | "ticket";

export function SessionWorkspace({ session, preferences, onBack, onRefresh }: { session: Session; preferences: Preferences; onBack: () => void; onRefresh: () => Promise<void> }) {
  const defaultTab: WorkTab = session.agent === "shell" || preferences.session_surface === "terminal" ? "terminal" : "review";
  const [tab, setTab] = useState<WorkTab>(defaultTab);
  const [rightOpen, setRightOpen] = useState(session.agent === "shell" || preferences.session_surface === "terminal");
  const [ticket, setTicket] = useState<Ticket | null>(null);
  const [panelError, setPanelError] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [output, setOutput] = useState("");
  const [busy, setBusy] = useState(false);
  const [streamVersion, setStreamVersion] = useState(0);
  const outputRef = useRef<HTMLDivElement>(null);
  const active = ["running", "starting", "waiting"].includes(session.status);
  const resumable = ["claude", "claude-code", "codex", "codex-cli"].includes(session.agent) && !active;
  const chatCapable = session.agent !== "shell";
  const canMessage = chatCapable && (active || resumable);

  useEffect(() => {
    setTab(defaultTab);
    setRightOpen(session.agent === "shell" || preferences.session_surface === "terminal");
    setPanelError(null);
    setTicket(null);
  }, [defaultTab, preferences.session_surface, session.agent, session.id]);

  useEffect(() => {
    setOutput("");
    const socket = new WebSocket(streamURL(session.id));
    socket.onmessage = (event) => {
      const message = JSON.parse(String(event.data)) as { type: string; data?: string };
      if (message.type === "output" && message.data) {
        setOutput((current) => (current + message.data!).slice(-2_000_000));
      }
    };
    return () => socket.close();
  }, [session.id, streamVersion]);

  useEffect(() => {
    outputRef.current?.scrollTo({ top: outputRef.current.scrollHeight, behavior: "smooth" });
  }, [output]);

  useEffect(() => {
    if (tab === "ticket" && session.ticket_key && !ticket) {
      void getTicket(session.ticket_key).then(setTicket).catch((reason) => setPanelError(String(reason)));
    }
  }, [session.ticket_key, tab, ticket]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!input.trim() || !canMessage) return;
    const value = input.trim();
    setInput("");
    try {
      if (active) {
        await sendInput(session.id, value + "\n");
      } else {
        setOutput((current) => `${current}\n${JSON.stringify({ type: "openade.user_message", text: value })}\n`);
        await sendMessage(session.id, value);
        setStreamVersion((current) => current + 1);
        await onRefresh();
      }
    } catch (reason) {
      setPanelError(reason instanceof Error ? reason.message : String(reason));
    }
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

  const openSurface = (surface: WorkTab) => {
    setTab(surface);
    setRightOpen(true);
  };

  return (
    <div className={`session-workspace ${rightOpen ? "with-panel" : ""} ${rightOpen && tab === "review" ? "panel-review" : ""} ${!chatCapable ? "shell-workspace" : ""}`}>
      <header className="session-header">
        <button className="icon-button" onClick={onBack} aria-label="Back"><ArrowLeft /></button>
        <span className={`status-dot ${session.status}`} />
        <div className="session-title"><h1>{session.title}</h1><p>{projectName(session.repo_root)} <span>·</span> <code title={session.branch}>{session.branch}</code></p></div>
        <StatusPill status={session.status} />
        {active && <button className="header-stop" onClick={() => void stopSession(session.id).then(onRefresh)}><Square weight="fill" /> Stop</button>}
        <nav className="session-work-actions" aria-label="Work surfaces">
          <SurfaceButton active={rightOpen && tab === "review"} onClick={() => openSurface("review")} icon={<GitDiff />} label="Changes" />
          <SurfaceButton active={rightOpen && tab === "terminal"} onClick={() => openSurface("terminal")} icon={<TerminalWindow />} label="Terminal" />
          <SurfaceButton active={rightOpen && tab === "pull-request"} onClick={() => openSurface("pull-request")} icon={<GithubLogo />} label="PR" />
        </nav>
        <button className="icon-button panel-toggle" onClick={() => setRightOpen((value) => !value)} aria-label={rightOpen ? "Close work panel" : "Open work panel"}><SidebarSimple /></button>
        <button className="icon-button" aria-label="Session actions"><DotsThree /></button>
      </header>
      <section className="conversation">
        <div className="messages" ref={outputRef}>
          {chatCapable ? <ChatTimeline session={session} output={output} activityExpanded={preferences.activity_detail === "expanded"} /> : <div className="shell-session-note"><TerminalWindow /><div><strong>Terminal run</strong><p>This run stays in the terminal so command output never gets mixed into chat.</p></div></div>}
        </div>
        {canMessage ? <form className="session-composer" onSubmit={submit}>
          <textarea
            value={input}
            onChange={(event) => setInput(event.target.value)}
            placeholder={canMessage ? `Message ${agentLabel(session.agent)}…` : "This run does not support follow-up messages"}
            rows={2}
            disabled={!canMessage}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                event.currentTarget.form?.requestSubmit();
              }
            }}
          />
          <div>
            <span className="runtime-chip"><span className={`status-dot ${session.status}`} />{active ? `${agentLabel(session.agent)} is attached` : resumable ? "Conversation can continue" : `Run ${session.status}`}</span>
            <button className="send-button" disabled={!canMessage || !input.trim()} aria-label="Send message"><ArrowUp weight="bold" /></button>
          </div>
        </form> : <div className="session-closed-state"><span className={`status-dot ${session.status}`} />{chatCapable ? `This ${agentLabel(session.agent)} run is ${session.status}` : "Use the Terminal panel to inspect this run"}</div>}
      </section>
      {rightOpen && (
        <aside className="work-panel">
          <div className="work-tabs">
            <TabButton active={tab === "review"} onClick={() => setTab("review")} icon={<GitDiff />} label="Changes" />
            <TabButton active={tab === "terminal"} onClick={() => setTab("terminal")} icon={<TerminalWindow />} label="Terminal" />
            <TabButton active={tab === "pull-request"} onClick={() => setTab("pull-request")} icon={<GithubLogo />} label="Pull request" />
            {chatCapable && <button className="icon-button" onClick={() => setRightOpen(false)} aria-label="Close work panel"><X /></button>}
          </div>
          <div className="panel-body">
            {panelError && <div className="inline-error">{panelError}</div>}
            {tab === "terminal" ? <TerminalWorkspace sessionId={session.id} /> : tab === "review" ? <ReviewWorkspace sessionId={session.id} /> : tab === "ticket" ? <TicketPanel ticket={ticket} session={session} /> : <PRPanel session={session} busy={busy} onCreate={createPR} onTicket={() => setTab("ticket")} />}
          </div>
        </aside>
      )}
    </div>
  );
}

function TabButton({ active, icon, label, onClick }: { active: boolean; icon: ReactNode; label: string; onClick: () => void }) {
  return <button className={active ? "active" : ""} onClick={onClick}>{icon}{label}</button>;
}

function SurfaceButton({ active, icon, label, onClick }: { active: boolean; icon: ReactNode; label: string; onClick: () => void }) {
  return <button className={active ? "active" : ""} onClick={onClick}>{icon}<span>{label}</span></button>;
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

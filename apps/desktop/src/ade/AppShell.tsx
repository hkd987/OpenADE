import {
  Pulse,
  ArrowUp,
  Check,
  Code,
  Command,
  Cpu,
  DotsThree,
  FileCode,
  GitBranch,
  GitDiff,
  GithubLogo,
  Lightning,
  ListMagnifyingGlass,
  Plus,
  Robot,
  SidebarSimple,
  SpinnerGap,
  TerminalWindow,
  Ticket as TicketIcon,
  X,
} from "@phosphor-icons/react";
import { FormEvent, useCallback, useEffect, useState } from "react";
import {
  createSession,
  ExternalConversation,
  getMeta,
  listProjects,
  listPullRequests,
  listSessions,
  scanWorkspace,
  Meta,
  projectName,
  PullRequest,
  relativeTime,
  Session,
} from "./api";
import { SessionWorkspace } from "./SessionWorkspace";
import { loadPreferences, Preferences, savePreferences, themeClass } from "./preferences";
import { SettingsPage } from "./SettingsPage";
import { Page, Sidebar } from "./Sidebar";
import { SitesPage } from "./SitesPage";

const agents = [
  { id: "claude", label: "Claude Code" },
  { id: "codex", label: "Codex CLI" },
  { id: "copilot", label: "Copilot CLI" },
  { id: "opencode", label: "OpenCode" },
  { id: "shell", label: "Local shell" },
];

const templates = [
  { title: "Implement a Jira ticket", category: "Delivery", icon: TicketIcon, prompt: "Read the linked ticket, inspect the repository, make the smallest correct change, run the relevant tests, and prepare a draft pull request." },
  { title: "Review a pull request", category: "Code review", icon: GitDiff, prompt: "Review the current branch for correctness, regressions, security issues, and missing tests. Report findings before making any edits." },
  { title: "Fix failing CI", category: "Maintenance", icon: Pulse, prompt: "Inspect the latest failing checks, reproduce the failure locally, fix the root cause, and verify the narrowest relevant test suite." },
  { title: "Add focused tests", category: "Quality", icon: Check, prompt: "Identify the important untested behavior in this ticket and add focused regression tests without unrelated production changes." },
  { title: "Explain this codebase", category: "Documentation", icon: FileCode, prompt: "Map the main modules, runtime boundaries, data flow, and development commands. Call out unclear ownership or risky coupling." },
  { title: "Dependency sweep", category: "Maintenance", icon: Code, prompt: "Find outdated dependencies and propose the smallest safe upgrade set. Avoid broad version churn and run compatibility checks." },
];

function AppShell() {
  const [page, setPage] = useState<Page>("home");
  const [sessions, setSessions] = useState<Session[]>([]);
  const [projects, setProjects] = useState<string[]>([]);
  const [scannedProjects, setScannedProjects] = useState<string[]>([]);
  const [externalConversations, setExternalConversations] = useState<ExternalConversation[]>([]);
  const [meta, setMeta] = useState<Meta | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [preferences, setPreferences] = useState<Preferences>(loadPreferences);
  const [error, setError] = useState<string | null>(null);
  const [connected, setConnected] = useState(false);
  const [resumingConversationId, setResumingConversationId] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [nextSessions, nextProjects, nextMeta] = await Promise.all([
        listSessions(),
        listProjects(),
        getMeta(),
      ]);
      setSessions(nextSessions);
      setProjects(nextProjects);
      setMeta(nextMeta);
      setConnected(true);
      setError(null);
    } catch (reason) {
      setConnected(false);
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1800);
    return () => window.clearInterval(timer);
  }, [refresh]);

  useEffect(() => {
    if (!preferences.project_root.trim()) {
      setScannedProjects([]);
      setExternalConversations([]);
      return;
    }
    void scanWorkspace(preferences.project_root)
      .then((result) => {
        setScannedProjects(result.projects);
        setExternalConversations(result.conversations);
      })
      .catch((reason) => setError(reason instanceof Error ? reason.message : String(reason)));
  }, [preferences.project_root]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        openComposer();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const selected = sessions.find((session) => session.id === selectedId) ?? null;
  const visibleProjects = [...new Set([...scannedProjects, ...projects])];
  const openSession = (id: string) => {
    setSelectedId(id);
    setPage("sessions");
  };
  const updatePreferences = (next: Preferences) => {
    setPreferences(next);
    savePreferences(next);
  };
  const openPage = (next: Page) => {
    setPage(next);
    setSelectedId(null);
  };
  const openComposer = () => {
    setPage("home");
    setSelectedId(null);
    window.setTimeout(() => document.querySelector<HTMLTextAreaElement>("[data-main-composer]")?.focus(), 0);
  };
  const resumeExternalConversation = async (conversation: ExternalConversation) => {
    if (resumingConversationId) return;
    setResumingConversationId(`${conversation.provider}:${conversation.id}`);
    setError(null);
    try {
      const session = await createSession({
        title: conversation.title,
        prompt: "",
        agent: conversation.provider,
        mode: "tui",
        resume_id: conversation.id,
        repo_root: conversation.project_root,
        base_branch: "HEAD",
      });
      await refresh();
      openSession(session.id);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setResumingConversationId(null);
    }
  };

  return (
    <div
      className={`ade ${themeClass(preferences.theme)} ${sidebarOpen ? "" : "sidebar-collapsed"}`}
      style={sidebarOpen ? undefined : { gridTemplateColumns: "minmax(0, 1fr)" }}
    >
      {sidebarOpen && <Sidebar
        page={page}
        sessions={sessions}
        projects={visibleProjects}
        externalConversations={externalConversations}
        projectOrganization={preferences.project_organization}
        projectSort={preferences.project_sort}
        resumingConversationId={resumingConversationId}
        selectedId={selectedId}
        connected={connected}
        onPage={openPage}
        onOpen={openSession}
        onResumeExternal={resumeExternalConversation}
        onProjectOrganization={(projectOrganization) => updatePreferences({ ...preferences, project_organization: projectOrganization })}
        onProjectSort={(projectSort) => updatePreferences({ ...preferences, project_sort: projectSort })}
        onNewSession={openComposer}
        onToggle={() => setSidebarOpen(false)}
      />}

      <main className="main-shell">
        {!sidebarOpen && <button className="sidebar-toggle icon-button" onClick={() => setSidebarOpen(true)} aria-label="Toggle sidebar"><SidebarSimple size={18} /></button>}
        {!connected && <div className="connection-banner"><SpinnerGap className="spin" /> Connecting to the local daemon… {error}</div>}
        {connected && error && <button className="error-toast" onClick={() => setError(null)}><span>{error}</span><X /></button>}
        {selected ? (
          <SessionWorkspace session={selected} preferences={preferences} onBack={() => setSelectedId(null)} onRefresh={refresh} />
        ) : page === "home" ? (
          <Home sessions={sessions} projects={visibleProjects} meta={meta} preferences={preferences} onCreated={(session) => { void refresh(); openSession(session.id); }} onOpen={openSession} onError={setError} />
        ) : page === "sites" ? (
          <SitesPage />
        ) : page === "sessions" ? (
          <SessionsPage sessions={sessions} onOpen={openSession} />
        ) : page === "agents" ? (
          <AgentsPage onUse={(prompt) => { sessionStorage.setItem("openade-template", prompt); setPage("home"); }} />
        ) : page === "review" ? (
          <ReviewPage projects={visibleProjects} sessions={sessions} />
        ) : (
          <SettingsPage preferences={preferences} onChange={updatePreferences} />
        )}
      </main>
    </div>
  );
}

function Home({ sessions, projects, meta, preferences, onCreated, onOpen, onError }: { sessions: Session[]; projects: string[]; meta: Meta | null; preferences: Preferences; onCreated: (session: Session) => void; onOpen: (id: string) => void; onError: (error: string | null) => void }) {
  const [prompt, setPrompt] = useState(() => sessionStorage.getItem("openade-template") ?? "");
  const [repo, setRepo] = useState(projects[0] ?? "");
  const [agent, setAgent] = useState(preferences.default_agent);
  const [ticket, setTicket] = useState("");
  const [ticketURL, setTicketURL] = useState("");
  const [base, setBase] = useState("main");
  const [optionsOpen, setOptionsOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => { if (!repo && projects[0]) setRepo(projects[0]); }, [projects, repo]);
  useEffect(() => { setAgent(preferences.default_agent); }, [preferences.default_agent]);
  useEffect(() => {
    const preferredAvailable = meta?.agents.find((item) => item.id === preferences.default_agent)?.available;
    const installed = meta?.agents.find((item) => item.available)?.id;
    if (preferredAvailable === false && installed) setAgent(installed);
  }, [meta, preferences.default_agent]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!prompt.trim() || !repo.trim()) return;
    setBusy(true);
    onError(null);
    try {
      const session = await createSession({ title: prompt.trim().split("\n")[0].slice(0, 68), prompt: prompt.trim(), agent, mode: preferences.session_surface === "terminal" && ["codex", "claude"].includes(agent) ? "tui" : "chat", repo_root: repo.trim(), base_branch: base.trim() || "HEAD", ticket_key: ticket.trim(), ticket_url: ticketURL.trim() });
      sessionStorage.removeItem("openade-template");
      onCreated(session);
    } catch (reason) {
      onError(reason instanceof Error ? reason.message : String(reason));
    } finally { setBusy(false); }
  };

  const browse = async () => {
    try {
      const selected = await selectRepository();
      if (selected) setRepo(selected);
    } catch (reason) {
      onError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const active = sessions.filter((item) => ["starting", "running", "waiting"].includes(item.status));
  const visibleSessions = active.length ? active.slice(0, 3) : sessions.slice(0, 3);
  return <div className="home-page">
    <div className="home-topline"><span className="eyebrow">Local agent workspace</span><span className="shortcut"><Command size={12} /> K to focus</span></div>
    <section className="home-hero">
      <div className="home-heading"><span className="pulse-mark"><Lightning weight="fill" /></span><div><h1>What should an agent handle?</h1><p>Each task gets a durable session, its own branch, and an isolated worktree.</p></div></div>
      <form className="composer" onSubmit={submit}>
        <textarea data-main-composer value={prompt} onChange={(event) => setPrompt(event.target.value)} placeholder="Ask to make changes, link a ticket, or investigate a pull request…" rows={4} />
        {optionsOpen && <div className="composer-options">
          <label><span>Jira key</span><input value={ticket} onChange={(event) => setTicket(event.target.value.toUpperCase())} placeholder="ADE-123" /></label>
          <label><span>Ticket URL</span><input value={ticketURL} onChange={(event) => setTicketURL(event.target.value)} placeholder="https://…/browse/ADE-123" /></label>
          <label><span>Base branch</span><input value={base} onChange={(event) => setBase(event.target.value)} placeholder="main" /></label>
        </div>}
        <div className="composer-toolbar">
          <div className="composer-left">
            <button type="button" className={`round-action ${optionsOpen ? "active" : ""}`} onClick={() => setOptionsOpen((value) => !value)}><Plus size={17} /></button>
            <div className="select-chip"><GitBranch size={15} /><input list="project-list" value={repo} onChange={(event) => setRepo(event.target.value)} placeholder="Choose repository" /><button type="button" className="browse-repo" onClick={browse}>Browse</button></div>
            <datalist id="project-list">{projects.map((project) => <option value={project} key={project} />)}</datalist>
          </div>
          <div className="composer-right">
            <label className="select-chip agent-chip"><Cpu size={15} /><select value={agent} onChange={(event) => setAgent(event.target.value)}>{agents.map((item) => <option value={item.id} key={item.id} disabled={item.id !== "shell" && meta?.agents.find((candidate) => candidate.id === item.id)?.available === false}>{item.label}</option>)}</select></label>
            <button className="send-button" type="submit" disabled={busy || !prompt.trim() || !repo.trim()}>{busy ? <SpinnerGap className="spin" /> : <ArrowUp weight="bold" />}</button>
          </div>
        </div>
      </form>
      <div className="trust-row"><span><Check /> Local-only transcripts</span><span><GitBranch /> Worktree isolated</span><span><TerminalWindow /> Bring your own CLI auth</span></div>
    </section>
    <section className="active-section">
      <div className="section-heading"><div><h2>{active.length ? "Active sessions" : "Recent work"}</h2><p>{active.length ? "Agents currently running or waiting for you" : "Continue a session without searching for it"}</p></div><span>{active.length ? `${active.length} live` : "Up to date"}</span></div>
      {visibleSessions.length ? <div className="active-grid">{visibleSessions.map((session) => <SessionCard key={session.id} session={session} onOpen={() => onOpen(session.id)} />)}</div> : <div className="quiet-empty"><strong>No sessions yet</strong><p>Describe a task above to create the first isolated worktree.</p></div>}
    </section>
  </div>;
}

function SessionCard({ session, onOpen }: { session: Session; onOpen: () => void }) {
  return <button className="session-card" onClick={onOpen}><div className="card-top"><span className={`status-dot ${session.status}`} /><span className="agent-label">{agentLabel(session.agent)}</span><DotsThree /></div><strong>{session.title}</strong><p>{projectName(session.repo_root)}</p><div className="card-bottom">{session.ticket_key ? <span className="ticket-chip"><TicketIcon />{session.ticket_key}</span> : <span />}<small>{relativeTime(session.updated_at)}</small></div></button>;
}

function SessionsPage({ sessions, onOpen }: { sessions: Session[]; onOpen: (id: string) => void }) {
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState("all");
  const filtered = sessions.filter((session) => (status === "all" || session.status === status) && `${session.title} ${session.repo_root} ${session.branch} ${session.ticket_key}`.toLowerCase().includes(query.toLowerCase()));
  return <div className="list-page"><PageHeader eyebrow="Workspace" title="Sessions" subtitle={`${sessions.length} indexed across ${new Set(sessions.map((item) => item.repo_root)).size} repositories`} />
    <div className="list-toolbar"><label className="search-box"><ListMagnifyingGlass /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search sessions" /></label><select value={status} onChange={(event) => setStatus(event.target.value)}><option value="all">All statuses</option><option value="running">Running</option><option value="waiting">Waiting</option><option value="completed">Completed</option><option value="failed">Failed</option></select></div>
    <div className="session-table"><div className="table-head"><span>Task</span><span>Project</span><span>Linked work</span><span>Status</span><span>Updated</span></div>{filtered.map((session) => <button className="table-row" key={session.id} onClick={() => onOpen(session.id)} title={`Branch: ${session.branch}`}><span className="task-cell"><span className={`status-dot ${session.status}`} /><strong>{session.title}</strong><small>{agentLabel(session.agent)}</small></span><span>{projectName(session.repo_root)}</span><span className="linked-work-cell">{session.ticket_key ? <span className="ticket-chip"><TicketIcon />{session.ticket_key}</span> : <small>No linked ticket</small>}</span><span><StatusPill status={session.status} /></span><span>{relativeTime(session.updated_at)}</span></button>)}{filtered.length === 0 && <div className="table-empty">No sessions match these filters.</div>}</div>
  </div>;
}

function AgentsPage({ onUse }: { onUse: (prompt: string) => void }) {
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState("All");
  const categories = ["All", ...new Set(templates.map((item) => item.category))];
  const filtered = templates.filter((item) => (category === "All" || item.category === category) && `${item.title} ${item.prompt}`.toLowerCase().includes(query.toLowerCase()));
  return <div className="agents-page"><div className="agents-hero"><div><span className="eyebrow">Reusable instructions</span><h1>Workflows</h1><p>Start from a focused brief, then choose the repository and linked work item.</p></div><div className="agent-constellation"><span><Robot /></span><i /><span><GithubLogo /></span><i /><span><TicketIcon /></span></div></div><label className="template-search"><ListMagnifyingGlass /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Find a workflow" /></label><div className="filter-chips">{categories.map((item) => <button className={category === item ? "active" : ""} onClick={() => setCategory(item)} key={item}>{item}</button>)}</div><div className="template-grid">{filtered.map((template) => { const Icon = template.icon; return <button key={template.title} className="template-card" onClick={() => onUse(template.prompt)}><div className="template-icon"><Icon /></div><strong>{template.title}</strong><p>{template.prompt}</p><span>Start with this workflow <ArrowUp /></span></button>; })}</div></div>;
}

function agentLabel(agent: string): string {
  return ({ claude: "Claude Code", codex: "Codex CLI", copilot: "Copilot", opencode: "OpenCode", shell: "Local shell" } as Record<string, string>)[agent] ?? agent;
}

function ReviewPage({ projects, sessions }: { projects: string[]; sessions: Session[] }) {
  const [repo, setRepo] = useState(projects[0] ?? "");
  const [prs, setPRs] = useState<PullRequest[]>([]);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { if (!repo && projects[0]) setRepo(projects[0]); }, [projects, repo]);
  useEffect(() => {
    if (!repo) return;
    setLoading(true); setError(null);
    void listPullRequests(repo).then(setPRs).catch((reason) => setError(reason instanceof Error ? reason.message : String(reason))).finally(() => setLoading(false));
  }, [repo]);
  const draft = prs.filter((pr) => pr.isDraft).length;
  const needsReview = prs.filter((pr) => pr.reviewDecision === "REVIEW_REQUIRED").length;
  const filtered = prs.filter((pr) => `${pr.title} ${pr.author.login} ${pr.headRefName}`.toLowerCase().includes(query.toLowerCase()));
  return <div className="review-page"><div className="review-top"><PageHeader eyebrow="GitHub" title="Pull requests" subtitle="Review and deliver changes across local projects" /><label className="repo-picker"><GithubLogo /><select value={repo} onChange={(event) => setRepo(event.target.value)}>{projects.map((project) => <option key={project} value={project}>{projectName(project)}</option>)}</select></label></div><div className="review-metrics"><Metric label="Open" value={prs.length} tone="green" /><Metric label="Needs review" value={needsReview} tone="orange" /><Metric label="Draft" value={draft} tone="neutral" /></div><div className="list-toolbar"><label className="search-box"><ListMagnifyingGlass /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search pull requests" /></label></div>{loading ? <div className="loading-state"><SpinnerGap className="spin" /> Loading pull requests…</div> : error ? <div className="inline-error">{error}</div> : <div className="pr-list">{filtered.map((pr) => { const linked = sessions.find((session) => session.branch === pr.headRefName); return <button key={pr.number} className="pr-row" onClick={() => window.open(pr.url, "_blank")}><span className="pr-number">#{pr.number}</span><span className="pr-main"><strong>{pr.title}</strong><small>{pr.headRefName} → {pr.baseRefName} · @{pr.author.login}</small></span>{linked?.ticket_key && <span className="ticket-chip"><TicketIcon />{linked.ticket_key}</span>}<StatusPill status={pr.isDraft ? "interrupted" : "running"} label={pr.isDraft ? "Draft" : "Open"} /></button>; })}{filtered.length === 0 && <div className="table-empty">No open pull requests found for this repository.</div>}</div>}</div>;
}

function Metric({ label, value, tone }: { label: string; value: number; tone: string }) { return <div className={`metric ${tone}`}><span>{label}</span><strong>{value}</strong></div>; }
function PageHeader({ eyebrow, title, subtitle }: { eyebrow: string; title: string; subtitle: string }) { return <header className="page-header"><span className="eyebrow">{eyebrow}</span><h1>{title}</h1><p>{subtitle}</p></header>; }
function StatusPill({ status, label }: { status: string; label?: string }) { return <span className={`status-pill ${status}`}><span className="status-dot" />{label ?? status.replace("-", " ")}</span>; }

async function selectRepository(): Promise<string> {
  const bridge = window as typeof window & {
    go?: { main?: { App?: { SelectRepository?: () => Promise<string> } } };
  };
  if (bridge.go?.main?.App?.SelectRepository) {
    return bridge.go.main.App.SelectRepository();
  }
  throw new Error("Folder selection is available in the Wails desktop build.");
}

export default AppShell;

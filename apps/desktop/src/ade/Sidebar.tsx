import {
  CaretDown,
  Folder,
  GitBranch,
  House,
  ListMagnifyingGlass,
  Plus,
  Robot,
  SidebarSimple,
  SlidersHorizontal,
  SpinnerGap,
  TerminalWindow,
} from "@phosphor-icons/react";
import { ReactNode, useEffect, useMemo, useState } from "react";
import { ExternalConversation, projectName, relativeTime, Session } from "./api";

export type Page = "home" | "sessions" | "agents" | "review" | "settings";

export function Sidebar({
  page,
  sessions,
  projects,
  externalConversations,
  resumingConversationId,
  selectedId,
  connected,
  onPage,
  onOpen,
  onResumeExternal,
  onNewSession,
  onToggle,
}: {
  page: Page;
  sessions: Session[];
  projects: string[];
  externalConversations: ExternalConversation[];
  resumingConversationId: string | null;
  selectedId: string | null;
  connected: boolean;
  onPage: (page: Page) => void;
  onOpen: (id: string) => void;
  onResumeExternal: (conversation: ExternalConversation) => void;
  onNewSession: () => void;
  onToggle: () => void;
}) {
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set(projects.slice(0, 3)));
  const [showAll, setShowAll] = useState<Set<string>>(new Set());
  const [showAllProjects, setShowAllProjects] = useState(false);
  const grouped = useMemo(() => {
    const roots = [...new Set([...projects, ...sessions.map((session) => session.repo_root), ...externalConversations.map((conversation) => conversation.project_root)])];
    return roots.map((root) => {
      const items = [
        ...sessions.filter((session) => session.repo_root === root).map((session) => ({ kind: "session" as const, updatedAt: session.updated_at, session })),
        ...externalConversations.filter((conversation) => conversation.project_root === root).map((conversation) => ({ kind: "external" as const, updatedAt: conversation.updated_at, conversation })),
      ].sort((left, right) => Date.parse(right.updatedAt) - Date.parse(left.updatedAt));
      return { root, items };
    }).sort((left, right) => {
      const leftUpdated = left.items[0]?.updatedAt;
      const rightUpdated = right.items[0]?.updatedAt;
      if (leftUpdated && rightUpdated) return Date.parse(rightUpdated) - Date.parse(leftUpdated);
      if (leftUpdated) return -1;
      if (rightUpdated) return 1;
      return projectName(left.root).localeCompare(projectName(right.root));
    });
  }, [externalConversations, projects, sessions]);

  const recents = useMemo(() => [
    ...sessions.map((session) => ({ kind: "session" as const, updatedAt: session.updated_at, session })),
    ...externalConversations.map((conversation) => ({ kind: "external" as const, updatedAt: conversation.updated_at, conversation })),
  ].sort((left, right) => Date.parse(right.updatedAt) - Date.parse(left.updatedAt)).slice(0, 7), [externalConversations, sessions]);
  const visibleGroups = showAllProjects ? grouped : grouped.slice(0, 10);

  useEffect(() => {
    setExpanded((current) => current.size ? current : new Set(grouped.slice(0, 3).map((group) => group.root)));
  }, [grouped]);

  const toggle = (root: string) => setExpanded((current) => {
    const next = new Set(current);
    if (next.has(root)) next.delete(root); else next.add(root);
    return next;
  });

  return (
    <aside className="sidebar">
      <div className="workspace-switcher"><div className="workspace-mark">O</div><span>OpenADE</span><button className="icon-button workspace-action" onClick={onNewSession} aria-label="New session" title="New session"><Plus /></button><button className="icon-button workspace-action" onClick={onToggle} aria-label="Collapse sidebar" title="Collapse sidebar"><SidebarSimple /></button></div>
      <nav className="primary-nav" aria-label="Primary">
        <NavButton icon={<House />} label="Home" active={page === "home"} onClick={() => onPage("home")} />
        <NavButton icon={<ListMagnifyingGlass />} label="Sessions" active={page === "sessions"} onClick={() => onPage("sessions")} />
        <NavButton icon={<Robot />} label="Workflows" active={page === "agents"} onClick={() => onPage("agents")} />
        <NavButton icon={<GitBranch />} label="Review" active={page === "review"} onClick={() => onPage("review")} />
      </nav>

      <div className="sidebar-scroll">
        <div className="sidebar-section-title">Projects</div>
        <div className="project-groups">
          {visibleGroups.map(({ root, items }) => {
            const open = expanded.has(root);
            const visible = showAll.has(root) ? items : items.slice(0, 3);
            return <section className="project-group" key={root}>
              <button className="project-row" onClick={() => toggle(root)} title={root} aria-expanded={open}><Folder /><span>{projectName(root)}</span><CaretDown className={open ? "open" : ""} /></button>
              {open && <div className="project-sessions">
                {visible.map((item) => item.kind === "session"
                  ? <button className={selectedId === item.session.id ? "active" : ""} key={`session:${item.session.id}`} onClick={() => onOpen(item.session.id)} title={item.session.title}><span className={`status-dot ${item.session.status}`} /><span>{item.session.title}</span></button>
                  : <ExternalConversationButton conversation={item.conversation} key={`external:${item.conversation.provider}:${item.conversation.id}`} busy={resumingConversationId === `${item.conversation.provider}:${item.conversation.id}`} onOpen={onResumeExternal} />)}
                {items.length > 3 && <button className="show-more" onClick={() => setShowAll((current) => { const next = new Set(current); if (next.has(root)) next.delete(root); else next.add(root); return next; })}>{showAll.has(root) ? "Show less" : `Show ${items.length - 3} more`}</button>}
              </div>}
            </section>;
          })}
          {grouped.length > 10 && <button className="all-projects-toggle" onClick={() => setShowAllProjects((value) => !value)}>{showAllProjects ? "Show fewer projects" : `Show ${grouped.length - 10} more projects`}</button>}
        </div>

        <div className="sidebar-section-title recent-title">Recents</div>
        <div className="recent-list">
          {recents.map((item) => item.kind === "session"
            ? <button key={`session:${item.session.id}`} className={`recent-item ${selectedId === item.session.id ? "active" : ""}`} onClick={() => onOpen(item.session.id)} title={item.session.title}><span className={`status-dot ${item.session.status}`} /><span className="recent-copy"><strong>{item.session.title}</strong><small>{relativeTime(item.session.updated_at)} · {projectName(item.session.repo_root)}</small></span></button>
            : <button key={`external:${item.conversation.provider}:${item.conversation.id}`} className="recent-item external-session" onClick={() => onResumeExternal(item.conversation)} disabled={Boolean(resumingConversationId)} title={`Resume this ${providerLabel(item.conversation.provider)} conversation`}><TerminalWindow /><span className="recent-copy"><strong>{item.conversation.title}</strong><small>{relativeTime(item.conversation.updated_at)} · {providerLabel(item.conversation.provider)}</small></span>{resumingConversationId === `${item.conversation.provider}:${item.conversation.id}` && <SpinnerGap className="spin" />}</button>)}
          {recents.length === 0 && <div className="recent-empty">Your active work will stay here.</div>}
        </div>
      </div>
      <div className="sidebar-footer"><div className="profile"><span className="avatar">KH</span><span><strong>Local workspace</strong><small>{connected ? "Daemon connected" : "Reconnecting…"}</small></span></div><button className={`icon-button ${page === "settings" ? "active" : ""}`} onClick={() => onPage("settings")} aria-label="Open settings" title="Settings"><SlidersHorizontal /></button></div>
    </aside>
  );
}

function ExternalConversationButton({ conversation, busy, onOpen }: { conversation: ExternalConversation; busy: boolean; onOpen: (conversation: ExternalConversation) => void }) {
  return <button className="external-session" onClick={() => onOpen(conversation)} disabled={busy} title={`Resume this ${providerLabel(conversation.provider)} conversation`}><TerminalWindow /> <span>{conversation.title}</span><small>{providerLabel(conversation.provider)} · {relativeTime(conversation.updated_at)}</small>{busy && <SpinnerGap className="spin" />}</button>;
}

function providerLabel(provider: ExternalConversation["provider"]): string {
  return provider === "claude" ? "Claude" : "Codex";
}

function NavButton({ icon, label, active, onClick }: { icon: ReactNode; label: string; active: boolean; onClick: () => void }) {
  return <button className={active ? "active" : ""} onClick={onClick}>{icon}<span>{label}</span></button>;
}

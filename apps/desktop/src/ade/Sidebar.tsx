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
} from "@phosphor-icons/react";
import { ReactNode, useEffect, useMemo, useState } from "react";
import { projectName, relativeTime, Session } from "./api";

export type Page = "home" | "sessions" | "agents" | "review" | "settings";

export function Sidebar({
  page,
  sessions,
  projects,
  selectedId,
  connected,
  onPage,
  onOpen,
  onNewSession,
  onToggle,
}: {
  page: Page;
  sessions: Session[];
  projects: string[];
  selectedId: string | null;
  connected: boolean;
  onPage: (page: Page) => void;
  onOpen: (id: string) => void;
  onNewSession: () => void;
  onToggle: () => void;
}) {
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set(projects.slice(0, 3)));
  const [showAll, setShowAll] = useState<Set<string>>(new Set());
  const grouped = useMemo(() => {
    const roots = [...new Set([...projects, ...sessions.map((session) => session.repo_root)])];
    return roots.map((root) => ({ root, sessions: sessions.filter((session) => session.repo_root === root) }));
  }, [projects, sessions]);

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
          {grouped.map(({ root, sessions: projectSessions }) => {
            const open = expanded.has(root);
            const visible = showAll.has(root) ? projectSessions : projectSessions.slice(0, 2);
            return <section className="project-group" key={root}>
              <button className="project-row" onClick={() => toggle(root)} title={root} aria-expanded={open}><Folder /><span>{projectName(root)}</span><CaretDown className={open ? "open" : ""} /></button>
              {open && <div className="project-sessions">
                {visible.map((session) => <button className={selectedId === session.id ? "active" : ""} key={session.id} onClick={() => onOpen(session.id)} title={session.title}><span className={`status-dot ${session.status}`} /><span>{session.title}</span></button>)}
                {projectSessions.length > 2 && <button className="show-more" onClick={() => setShowAll((current) => { const next = new Set(current); if (next.has(root)) next.delete(root); else next.add(root); return next; })}>{showAll.has(root) ? "Show less" : `Show ${projectSessions.length - 2} more`}</button>}
              </div>}
            </section>;
          })}
        </div>

        <div className="sidebar-section-title recent-title">Recents</div>
        <div className="recent-list">
          {sessions.slice(0, 7).map((session) => <button key={session.id} className={`recent-item ${selectedId === session.id ? "active" : ""}`} onClick={() => onOpen(session.id)} title={session.title}><span className={`status-dot ${session.status}`} /><span className="recent-copy"><strong>{session.title}</strong><small>{relativeTime(session.updated_at)} · {projectName(session.repo_root)}</small></span></button>)}
          {sessions.length === 0 && <div className="recent-empty">Your active work will stay here.</div>}
        </div>
      </div>
      <div className="sidebar-footer"><div className="profile"><span className="avatar">KH</span><span><strong>Local workspace</strong><small>{connected ? "Daemon connected" : "Reconnecting…"}</small></span></div><button className={`icon-button ${page === "settings" ? "active" : ""}`} onClick={() => onPage("settings")} aria-label="Open settings" title="Settings"><SlidersHorizontal /></button></div>
    </aside>
  );
}

function NavButton({ icon, label, active, onClick }: { icon: ReactNode; label: string; active: boolean; onClick: () => void }) {
  return <button className={active ? "active" : ""} onClick={onClick}>{icon}<span>{label}</span></button>;
}

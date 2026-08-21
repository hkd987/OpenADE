import {
  CaretDown,
  Check,
  DotsThree,
  Folder,
  GitBranch,
  House,
  ListMagnifyingGlass,
  Plus,
  Robot,
  SidebarSimple,
  SlidersHorizontal,
  SquaresFour,
  SpinnerGap,
  TerminalWindow,
} from "@phosphor-icons/react";
import { ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { ExternalConversation, projectName, relativeTime, Session } from "./api";
import { ProjectOrganization, ProjectSort } from "./preferences";

export type Page = "home" | "sites" | "sessions" | "agents" | "review" | "settings";

type SidebarItem =
  | { kind: "session"; updatedAt: string; session: Session }
  | { kind: "external"; updatedAt: string; conversation: ExternalConversation };

export function Sidebar({
  page,
  sessions,
  projects,
  externalConversations,
  projectOrganization,
  projectSort,
  resumingConversationId,
  selectedId,
  connected,
  onPage,
  onOpen,
  onResumeExternal,
  onProjectOrganization,
  onProjectSort,
  onNewSession,
  onToggle,
}: {
  page: Page;
  sessions: Session[];
  projects: string[];
  externalConversations: ExternalConversation[];
  projectOrganization: ProjectOrganization;
  projectSort: ProjectSort;
  resumingConversationId: string | null;
  selectedId: string | null;
  connected: boolean;
  onPage: (page: Page) => void;
  onOpen: (id: string) => void;
  onResumeExternal: (conversation: ExternalConversation) => void;
  onProjectOrganization: (organization: ProjectOrganization) => void;
  onProjectSort: (sort: ProjectSort) => void;
  onNewSession: () => void;
  onToggle: () => void;
}) {
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set(projects.slice(0, 3)));
  const [showAll, setShowAll] = useState<Set<string>>(new Set());
  const [showAllProjects, setShowAllProjects] = useState(false);
  const [projectsCollapsed, setProjectsCollapsed] = useState(false);
  const [projectMenuOpen, setProjectMenuOpen] = useState(false);
  const projectMenuRef = useRef<HTMLDivElement>(null);

  const allItems = useMemo<SidebarItem[]>(() => [
    ...sessions.map((session) => ({ kind: "session" as const, updatedAt: session.updated_at, session })),
    ...externalConversations.map((conversation) => ({ kind: "external" as const, updatedAt: conversation.updated_at, conversation })),
  ], [externalConversations, sessions]);

  const grouped = useMemo(() => {
    const roots = [...new Set([
      ...projects,
      ...sessions.map((session) => session.repo_root),
      ...externalConversations.map((conversation) => conversation.project_root),
    ])];
    const groups = roots.map((root) => ({
      root,
      items: sortSidebarItems(allItems.filter((item) => itemRoot(item) === root), projectSort),
    }));
    if (projectSort === "manual") return groups;
    return groups.sort((left, right) => {
      const leftFirst = left.items[0];
      const rightFirst = right.items[0];
      if (leftFirst && rightFirst) {
        const compared = compareSidebarItems(leftFirst, rightFirst, projectSort);
        if (compared) return compared;
      }
      if (leftFirst) return -1;
      if (rightFirst) return 1;
      return projectName(left.root).localeCompare(projectName(right.root));
    });
  }, [allItems, externalConversations, projectSort, projects, sessions]);

  const sortedItems = useMemo(() => sortSidebarItems(allItems, projectSort), [allItems, projectSort]);
  const recents = useMemo(() => sortSidebarItems(allItems, "updated").slice(0, 7), [allItems]);
  const visibleGroups = showAllProjects ? grouped : grouped.slice(0, 10);
  const visibleFlatItems = showAllProjects ? sortedItems : sortedItems.slice(0, 10);

  useEffect(() => {
    setExpanded((current) => current.size ? current : new Set(grouped.slice(0, 3).map((group) => group.root)));
  }, [grouped]);

  useEffect(() => {
    if (!projectMenuOpen) return;
    const closeOnOutsidePointer = (event: PointerEvent) => {
      if (!projectMenuRef.current?.contains(event.target as Node)) setProjectMenuOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setProjectMenuOpen(false);
    };
    document.addEventListener("pointerdown", closeOnOutsidePointer);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsidePointer);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [projectMenuOpen]);

  const toggle = (root: string) => setExpanded((current) => {
    const next = new Set(current);
    if (next.has(root)) next.delete(root); else next.add(root);
    return next;
  });

  const chooseOrganization = (organization: ProjectOrganization) => {
    onProjectOrganization(organization);
    setShowAllProjects(false);
    setProjectMenuOpen(false);
  };

  const chooseSort = (sort: ProjectSort) => {
    onProjectSort(sort);
    setProjectMenuOpen(false);
  };

  return (
    <aside className="sidebar">
      <div className="workspace-switcher"><div className="workspace-mark">O</div><span>OpenADE</span><button className="icon-button workspace-action" onClick={onNewSession} aria-label="New session" title="New session"><Plus /></button><button className="icon-button workspace-action" onClick={onToggle} aria-label="Collapse sidebar" title="Collapse sidebar"><SidebarSimple /></button></div>
      <nav className="primary-nav" aria-label="Primary">
        <NavButton icon={<House />} label="Home" active={page === "home"} onClick={() => onPage("home")} />
        <NavButton icon={<SquaresFour />} label="Sites" active={page === "sites"} onClick={() => onPage("sites")} />
        <NavButton icon={<ListMagnifyingGlass />} label="Sessions" active={page === "sessions"} onClick={() => onPage("sessions")} />
        <NavButton icon={<Robot />} label="Workflows" active={page === "agents"} onClick={() => onPage("agents")} />
        <NavButton icon={<GitBranch />} label="Review" active={page === "review"} onClick={() => onPage("review")} />
      </nav>

      <div className="sidebar-scroll">
        <div className="projects-heading" ref={projectMenuRef}>
          <button className="projects-toggle" onClick={() => setProjectsCollapsed((value) => !value)} aria-expanded={!projectsCollapsed}>
            <span>Projects</span><CaretDown className={projectsCollapsed ? "collapsed" : ""} />
          </button>
          <div className="projects-actions">
            <button onClick={() => setProjectMenuOpen((value) => !value)} aria-label="Project display settings" aria-haspopup="menu" aria-expanded={projectMenuOpen} title="Organize projects"><DotsThree /></button>
            <button onClick={() => onPage("settings")} aria-label="Add project" title="Add a workspace folder"><Plus /></button>
          </div>
          {projectMenuOpen && <ProjectMenu organization={projectOrganization} sort={projectSort} onOrganization={chooseOrganization} onSort={chooseSort} />}
        </div>

        {!projectsCollapsed && projectOrganization === "project" && <div className="project-groups">
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
        </div>}

        {!projectsCollapsed && projectOrganization === "list" && <div className="project-flat-list">
          {visibleFlatItems.map((item) => <ProjectFlatItem item={item} key={itemKey(item)} selectedId={selectedId} resumingConversationId={resumingConversationId} onOpen={onOpen} onResumeExternal={onResumeExternal} />)}
          {sortedItems.length > 10 && <button className="all-projects-toggle" onClick={() => setShowAllProjects((value) => !value)}>{showAllProjects ? "Show fewer chats" : `Show ${sortedItems.length - 10} more chats`}</button>}
          {sortedItems.length === 0 && <div className="project-empty">Chats from indexed projects will appear here.</div>}
        </div>}

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

function ProjectMenu({ organization, sort, onOrganization, onSort }: { organization: ProjectOrganization; sort: ProjectSort; onOrganization: (organization: ProjectOrganization) => void; onSort: (sort: ProjectSort) => void }) {
  return <div className="project-menu" role="menu" aria-label="Project display settings">
    <div className="project-menu-label">Organize sidebar</div>
    <MenuChoice label="By project" selected={organization === "project"} onClick={() => onOrganization("project")} />
    <MenuChoice label="In one list" selected={organization === "list"} onClick={() => onOrganization("list")} />
    <div className="project-menu-label project-sort-label">Sort chats by</div>
    <MenuChoice label="Priority" selected={sort === "priority"} onClick={() => onSort("priority")} />
    <MenuChoice label="Last updated" selected={sort === "updated"} onClick={() => onSort("updated")} />
    <MenuChoice label="Manual order" selected={sort === "manual"} onClick={() => onSort("manual")} />
  </div>;
}

function MenuChoice({ label, selected, onClick }: { label: string; selected: boolean; onClick: () => void }) {
  return <button className="project-menu-choice" role="menuitemradio" aria-checked={selected} onClick={onClick}><span className="project-menu-check">{selected && <Check weight="bold" />}</span><span>{label}</span></button>;
}

function ProjectFlatItem({ item, selectedId, resumingConversationId, onOpen, onResumeExternal }: { item: SidebarItem; selectedId: string | null; resumingConversationId: string | null; onOpen: (id: string) => void; onResumeExternal: (conversation: ExternalConversation) => void }) {
  if (item.kind === "session") {
    return <button className={`project-flat-item ${selectedId === item.session.id ? "active" : ""}`} onClick={() => onOpen(item.session.id)} title={item.session.title}><span className={`status-dot ${item.session.status}`} /><span><strong>{item.session.title}</strong><small>{projectName(item.session.repo_root)}</small></span></button>;
  }
  const busy = resumingConversationId === `${item.conversation.provider}:${item.conversation.id}`;
  return <button className="project-flat-item external-session" onClick={() => onResumeExternal(item.conversation)} disabled={busy} title={`Resume this ${providerLabel(item.conversation.provider)} conversation`}><TerminalWindow /><span><strong>{item.conversation.title}</strong><small>{projectName(item.conversation.project_root)} · {providerLabel(item.conversation.provider)}</small></span>{busy && <SpinnerGap className="spin" />}</button>;
}

function ExternalConversationButton({ conversation, busy, onOpen }: { conversation: ExternalConversation; busy: boolean; onOpen: (conversation: ExternalConversation) => void }) {
  return <button className="external-session" onClick={() => onOpen(conversation)} disabled={busy} title={`Resume this ${providerLabel(conversation.provider)} conversation`}><TerminalWindow /> <span>{conversation.title}</span><small>{providerLabel(conversation.provider)} · {relativeTime(conversation.updated_at)}</small>{busy && <SpinnerGap className="spin" />}</button>;
}

function sortSidebarItems(items: SidebarItem[], sort: ProjectSort): SidebarItem[] {
  if (sort === "manual") return [...items];
  return [...items].sort((left, right) => compareSidebarItems(left, right, sort));
}

function compareSidebarItems(left: SidebarItem, right: SidebarItem, sort: ProjectSort): number {
  if (sort === "priority") {
    const priority = itemPriority(left) - itemPriority(right);
    if (priority) return priority;
  }
  return timestamp(right.updatedAt) - timestamp(left.updatedAt);
}

function itemPriority(item: SidebarItem): number {
  if (item.kind === "external") return 2;
  return ["starting", "running", "waiting"].includes(item.session.status) ? 0 : 1;
}

function itemRoot(item: SidebarItem): string {
  return item.kind === "session" ? item.session.repo_root : item.conversation.project_root;
}

function itemKey(item: SidebarItem): string {
  return item.kind === "session" ? `session:${item.session.id}` : `external:${item.conversation.provider}:${item.conversation.id}`;
}

function timestamp(value: string): number {
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? 0 : parsed;
}

function providerLabel(provider: ExternalConversation["provider"]): string {
  return provider === "claude" ? "Claude" : "Codex";
}

function NavButton({ icon, label, active, onClick }: { icon: ReactNode; label: string; active: boolean; onClick: () => void }) {
  return <button className={active ? "active" : ""} onClick={onClick}>{icon}<span>{label}</span></button>;
}

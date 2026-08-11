import { projectName, SessionMeta, SessionState, timeAgo } from "../api";

/**
 * Projects view: one card per repository with per-state session counts and
 * last activity — the "step back and see everything" page next to the
 * session-centric grid.
 */
export function ProjectsView({
  projects,
  onNewSession,
  onOpenProject,
}: {
  projects: { repoRoot: string; sessions: SessionMeta[] }[];
  onNewSession: (repoRoot: string) => void;
  onOpenProject: (repoRoot: string) => void;
}) {
  const countsFor = (sessions: SessionMeta[]) => {
    const counts = new Map<SessionState, number>();
    for (const s of sessions) {
      counts.set(s.state, (counts.get(s.state) ?? 0) + 1);
    }
    return counts;
  };

  return (
    <section
      className="projects-view"
      aria-label="Projects"
      data-testid="projects-view"
    >
      {projects.length === 0 && (
        <p className="empty">No projects yet — launch a session to add one.</p>
      )}
      {projects.map((project) => {
        const counts = countsFor(project.sessions);
        const latest = project.sessions
          .map((s) => s.updated_at)
          .sort()
          .at(-1);
        return (
          <div
            key={project.repoRoot}
            className="project-card"
            data-testid="project-card"
          >
            <button
              type="button"
              className="project-card-main"
              title={project.repoRoot}
              onClick={() => onOpenProject(project.repoRoot)}
            >
              <span className="project-card-name">
                {projectName(project.repoRoot)}
              </span>
              <span className="project-card-path">{project.repoRoot}</span>
              <span className="project-card-counts">
                {(
                  [
                    "running",
                    "needs-input",
                    "completed",
                    "failed",
                  ] as SessionState[]
                )
                  .filter((state) => (counts.get(state) ?? 0) > 0)
                  .map((state) => (
                    <span key={state} className={`state state-${state}`}>
                      {counts.get(state)} {state}
                    </span>
                  ))}
                {latest !== undefined && timeAgo(latest) !== "" && (
                  <span className="project-card-age">
                    active {timeAgo(latest)}
                  </span>
                )}
              </span>
            </button>
            <button
              type="button"
              className="project-add"
              title={`New session in ${projectName(project.repoRoot)}`}
              data-testid="project-card-add"
              onClick={() => onNewSession(project.repoRoot)}
            >
              +
            </button>
          </div>
        );
      })}
    </section>
  );
}

import { useEffect, useState } from "react";
import {
  listPrs,
  PrInfo,
  projectName,
  SessionMeta,
  SessionState,
  timeAgo,
} from "../api";

/** Open PRs for one project, via the daemon's gh-backed endpoint. */
function ProjectPrs({ repoRoot }: { repoRoot: string }) {
  const [prs, setPrs] = useState<PrInfo[]>([]);
  useEffect(() => {
    listPrs(repoRoot)
      .then(({ prs }) => setPrs(prs))
      .catch(() => setPrs([]));
  }, [repoRoot]);
  if (prs.length === 0) {
    return null;
  }
  return (
    <div className="project-prs" data-testid="project-prs">
      <span className="project-prs-count">
        {prs.length} open PR{prs.length === 1 ? "" : "s"}
      </span>
      {prs.slice(0, 3).map((pr) => (
        <a key={pr.number} href={pr.url} target="_blank" rel="noreferrer">
          #{pr.number} {pr.title}
          {pr.isDraft ? " (draft)" : ""}
        </a>
      ))}
    </div>
  );
}

/**
 * Projects view: one card per repository with per-state session counts and
 * last activity — the "step back and see everything" page next to the
 * session-centric grid.
 */
export function ProjectsView({
  projects,
  onNewSession,
  onOpenProject,
  onGoal,
}: {
  projects: { repoRoot: string; sessions: SessionMeta[] }[];
  onNewSession: (repoRoot: string) => void;
  onOpenProject: (repoRoot: string) => void;
  /** Goal box: describe an outcome, a session launches immediately. */
  onGoal: (repoRoot: string, goal: string) => void;
}) {
  const [goals, setGoals] = useState<Record<string, string>>({});
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
            <form
              className="goal-box"
              onSubmit={(e) => {
                e.preventDefault();
                const goal = (goals[project.repoRoot] ?? "").trim();
                if (goal !== "") {
                  onGoal(project.repoRoot, goal);
                  setGoals((g) => ({ ...g, [project.repoRoot]: "" }));
                }
              }}
            >
              <input
                value={goals[project.repoRoot] ?? ""}
                onChange={(e) =>
                  setGoals((g) => ({
                    ...g,
                    [project.repoRoot]: e.target.value,
                  }))
                }
                placeholder="Describe a goal and press Enter to launch…"
                data-testid="goal-box"
              />
            </form>
            <ProjectPrs repoRoot={project.repoRoot} />
          </div>
        );
      })}
    </section>
  );
}

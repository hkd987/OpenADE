import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionMeta } from "../api";

const { listPrs } = vi.hoisted(() => ({ listPrs: vi.fn() }));
vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  listPrs,
}));
import { ProjectsView } from "./ProjectsView";

const mk = (over: Partial<SessionMeta>): SessionMeta => ({
  id: "s",
  title: "t",
  harness: "claude-code",
  repo_root: "/repos/checkout",
  state: "running",
  created_at: "2026-08-11T10:00:00Z",
  updated_at: new Date().toISOString(),
  ...over,
});

describe("ProjectsView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listPrs.mockResolvedValue({ prs: [] });
  });

  it("shows per-project state counts and last activity", () => {
    render(
      <ProjectsView
        projects={[
          {
            repoRoot: "/repos/checkout",
            sessions: [
              mk({ id: "a", state: "running" }),
              mk({ id: "b", state: "running" }),
              mk({ id: "c", state: "failed" }),
            ],
          },
        ]}
        onNewSession={() => {}}
        onOpenProject={() => {}}
        onGoal={() => {}}
      />,
    );
    const card = screen.getByTestId("project-card");
    expect(card).toHaveTextContent("checkout");
    expect(card).toHaveTextContent("2 running");
    expect(card).toHaveTextContent("1 failed");
    expect(card).not.toHaveTextContent("completed");
    expect(card).toHaveTextContent("active now");
  });

  it("wires the + and card actions", async () => {
    const onNewSession = vi.fn();
    const onOpenProject = vi.fn();
    render(
      <ProjectsView
        projects={[{ repoRoot: "/repos/checkout", sessions: [mk({})] }]}
        onNewSession={onNewSession}
        onOpenProject={onOpenProject}
        onGoal={() => {}}
      />,
    );
    await userEvent.click(screen.getByTestId("project-card-add"));
    expect(onNewSession).toHaveBeenCalledWith("/repos/checkout");
    await userEvent.click(screen.getByText("checkout"));
    expect(onOpenProject).toHaveBeenCalledWith("/repos/checkout");
  });

  it("renders an empty state without projects", () => {
    render(
      <ProjectsView
        projects={[]}
        onNewSession={() => {}}
        onOpenProject={() => {}}
        onGoal={() => {}}
      />,
    );
    expect(screen.getByTestId("projects-view")).toHaveTextContent(
      "No projects yet",
    );
  });

  it("goal box launches a described outcome and clears", async () => {
    const onGoal = vi.fn();
    render(
      <ProjectsView
        projects={[{ repoRoot: "/repos/checkout", sessions: [mk({})] }]}
        onNewSession={() => {}}
        onOpenProject={() => {}}
        onGoal={onGoal}
      />,
    );
    const box = screen.getByTestId("goal-box");
    await userEvent.type(box, "add retry budget metrics{Enter}");
    expect(onGoal).toHaveBeenCalledWith("/repos/checkout", "add retry budget metrics");
    expect(box).toHaveValue("");
    // Empty input does not launch.
    await userEvent.type(box, "   {Enter}");
    expect(onGoal).toHaveBeenCalledTimes(1);
  });

  it("shows open pull requests from the gh-backed endpoint", async () => {
    listPrs.mockResolvedValue({
      prs: [
        { number: 7, title: "Add retries", url: "https://github.com/a/x/pull/7", headRefName: "r", isDraft: false },
        { number: 8, title: "Draft work", url: "https://github.com/a/x/pull/8", headRefName: "d", isDraft: true },
      ],
    });
    render(
      <ProjectsView
        projects={[{ repoRoot: "/repos/checkout", sessions: [mk({})] }]}
        onNewSession={() => {}}
        onOpenProject={() => {}}
        onGoal={() => {}}
      />,
    );
    const prs = await screen.findByTestId("project-prs");
    expect(prs).toHaveTextContent("2 open PRs");
    expect(prs).toHaveTextContent("#7 Add retries");
    expect(prs).toHaveTextContent("#8 Draft work (draft)");
  });

  it("hides the PR section when the endpoint errors", async () => {
    listPrs.mockRejectedValue(new Error("down"));
    render(
      <ProjectsView
        projects={[{ repoRoot: "/repos/checkout", sessions: [mk({})] }]}
        onNewSession={() => {}}
        onOpenProject={() => {}}
        onGoal={() => {}}
      />,
    );
    await screen.findByTestId("project-card");
    expect(screen.queryByTestId("project-prs")).toBeNull();
  });

  it("handles a single PR and a project with no sessions yet", async () => {
    listPrs.mockResolvedValue({
      prs: [{ number: 7, title: "One", url: "https://x/pull/7", headRefName: "r", isDraft: false }],
    });
    render(
      <ProjectsView
        projects={[{ repoRoot: "/repos/checkout", sessions: [] }]}
        onNewSession={() => {}}
        onOpenProject={() => {}}
        onGoal={() => {}}
      />,
    );
    expect(await screen.findByTestId("project-prs")).toHaveTextContent("1 open PR");
    expect(screen.getByTestId("project-card")).not.toHaveTextContent("active");
  });
});

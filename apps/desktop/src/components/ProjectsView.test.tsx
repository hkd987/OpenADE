import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SessionMeta } from "../api";
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
      />,
    );
    expect(screen.getByTestId("projects-view")).toHaveTextContent(
      "No projects yet",
    );
  });
});

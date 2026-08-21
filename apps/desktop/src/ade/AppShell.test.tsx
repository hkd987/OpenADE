import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import AppShell from "./AppShell";

vi.mock("./api", async (loadOriginal) => {
  const original = await loadOriginal<typeof import("./api")>();
  return {
    ...original,
    getMeta: vi.fn().mockResolvedValue({
      agents: [
        { id: "claude", available: true, path: "/usr/local/bin/claude" },
        { id: "codex", available: true, path: "/usr/local/bin/codex" },
      ],
      github_available: true,
      data_dir: "/tmp/openade-test",
    }),
    listProjects: vi.fn().mockResolvedValue(["/tmp/example-repo"]),
    listSessions: vi.fn().mockResolvedValue([]),
  };
});

afterEach(() => {
  sessionStorage.clear();
});

describe("Tembo-inspired application shell", () => {
  it("presents the task composer and durable-session promise", async () => {
    render(<AppShell />);
    expect(
      screen.getByRole("heading", { name: "What should an agent handle?" }),
    ).toBeInTheDocument();
    expect(
      screen.getByPlaceholderText(/Ask to make changes/),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByText("Daemon connected")).toBeInTheDocument(),
    );
    expect(screen.getByText("Worktree isolated")).toBeInTheDocument();
  });

  it("moves from navigation into reusable agent templates", async () => {
    const user = userEvent.setup();
    render(<AppShell />);
    await user.click(screen.getByRole("button", { name: "Agents" }));
    expect(
      screen.getByRole("heading", { name: "Agent templates" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Implement a Jira ticket")).toBeInTheDocument();
    expect(screen.getByText("Fix failing CI")).toBeInTheDocument();
  });
});


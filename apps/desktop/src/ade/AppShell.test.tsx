import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import AppShell from "./AppShell";
import { createSession, scanWorkspace } from "./api";

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
    scanWorkspace: vi.fn().mockResolvedValue({ root: "/tmp", projects: [], conversations: [] }),
    createSession: vi.fn(),
  };
});

beforeEach(() => {
  const values = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
    clear: () => values.clear(),
  });
});

afterEach(() => {
  sessionStorage.clear();
  localStorage.clear();
  vi.unstubAllGlobals();
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
    await user.click(screen.getByRole("button", { name: "Workflows" }));
    expect(
      screen.getByRole("heading", { name: "Workflows" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Implement a Jira ticket")).toBeInTheDocument();
    expect(screen.getByText("Fix failing CI")).toBeInTheDocument();
  });

  it("persists a desktop theme from settings", async () => {
    const user = userEvent.setup();
    const { container } = render(<AppShell />);
    await user.click(screen.getByRole("button", { name: "Open settings" }));
    expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Glass/ }));
    expect(container.querySelector(".ade")).toHaveClass("theme-glass");
    expect(JSON.parse(localStorage.getItem("openade.preferences") ?? "{}").theme).toBe("glass");
  });

  it("shows local provider history under its project and resumes it in a direct TUI", async () => {
    localStorage.setItem("openade.preferences", JSON.stringify({ project_root: "/tmp" }));
    vi.mocked(scanWorkspace).mockResolvedValue({
      root: "/tmp",
      projects: ["/tmp/example-repo"],
      conversations: [{ id: "codex-history", provider: "codex", title: "Continue the parser cleanup", cwd: "/tmp/example-repo", project_root: "/tmp/example-repo", updated_at: new Date().toISOString() }],
    });
    vi.mocked(createSession).mockResolvedValue({ id: "imported", title: "Continue the parser cleanup", prompt: "", agent: "codex", mode: "tui", repo_root: "/tmp/example-repo", worktree_path: "/tmp/worktree", branch: "openade/imported", base_branch: "HEAD", status: "running", created_at: new Date().toISOString(), updated_at: new Date().toISOString() });
    const user = userEvent.setup();
    render(<AppShell />);
    const history = await screen.findAllByRole("button", { name: /Continue the parser cleanup/ });
    await user.click(history[0]);
    await waitFor(() => expect(createSession).toHaveBeenCalledWith(expect.objectContaining({ agent: "codex", mode: "tui", resume_id: "codex-history", repo_root: "/tmp/example-repo" })));
  });
});

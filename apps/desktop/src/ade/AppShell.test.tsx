import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import AppShell from "./AppShell";
import { createSession, listSessions, scanWorkspace, switchSessionSurface } from "./api";

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
    switchSessionSurface: vi.fn(),
  };
});

beforeEach(() => {
  vi.clearAllMocks();
  HTMLElement.prototype.scrollTo = vi.fn();
  const values = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
    clear: () => values.clear(),
  });
});

afterEach(() => {
  vi.useRealTimers();
  sessionStorage.clear();
  localStorage.clear();
  vi.unstubAllGlobals();
});

describe("Tembo-inspired application shell", () => {
  it("serializes daemon polling and stops scheduling after unmount", async () => {
    vi.useFakeTimers();
    let resolveSessions!: (sessions: Awaited<ReturnType<typeof listSessions>>) => void;
    vi.mocked(listSessions).mockImplementationOnce(() => new Promise((resolve) => {
      resolveSessions = resolve;
    }));

    const view = render(<AppShell />);
    expect(listSessions).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_400);
    });
    expect(listSessions).toHaveBeenCalledTimes(1);

    view.unmount();
    await act(async () => {
      resolveSessions([]);
      await Promise.resolve();
      await vi.runAllTimersAsync();
    });
    expect(listSessions).toHaveBeenCalledTimes(1);
  });

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

  it("removes all sidebar chrome when the sidebar is collapsed", async () => {
    const user = userEvent.setup();
    const { container } = render(<AppShell />);
    expect(container.querySelector(".sidebar")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Collapse sidebar" }));

    expect(container.querySelector(".ade")).toHaveClass("sidebar-collapsed");
    expect(container.querySelector(".sidebar")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Toggle sidebar" })).toBeInTheDocument();
    expect(getComputedStyle(container.querySelector(".ade")!).gridTemplateColumns).toBe("minmax(0, 1fr)");
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

  it("opens the presentation-only Sites surface from primary navigation", async () => {
    const user = userEvent.setup();
    render(<AppShell />);
    await user.click(screen.getByRole("button", { name: "Sites" }));
    expect(screen.getByRole("heading", { name: "Sites" })).toBeInTheDocument();
    const search = screen.getByPlaceholderText("Search sites");
    await user.type(search, "portfolio");
    expect(search).toHaveValue("portfolio");
    expect(screen.getByRole("heading", { name: "No sites yet" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create new site" })).toBeInTheDocument();
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

  it("persists project organization and chat sorting from the sidebar menu", async () => {
    const user = userEvent.setup();
    render(<AppShell />);

    await user.click(screen.getByRole("button", { name: "Project display settings" }));
    expect(screen.getByRole("menu", { name: "Project display settings" })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: "By project" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("menuitemradio", { name: "Priority" })).toHaveAttribute("aria-checked", "true");

    await user.click(screen.getByRole("menuitemradio", { name: "In one list" }));
    expect(JSON.parse(localStorage.getItem("openade.preferences") ?? "{}").project_organization).toBe("list");

    await user.click(screen.getByRole("button", { name: "Project display settings" }));
    await user.click(screen.getByRole("menuitemradio", { name: "Last updated" }));
    expect(JSON.parse(localStorage.getItem("openade.preferences") ?? "{}").project_sort).toBe("updated");
    expect(screen.getByRole("button", { name: "Add project" })).toBeInTheDocument();
  });

  it.each([
    { surface: "chat", expectedMode: "chat" },
    { surface: "terminal", expectedMode: "tui" },
  ] as const)("resumes indexed provider history using the $surface preference", async ({ surface, expectedMode }) => {
    localStorage.setItem("openade.preferences", JSON.stringify({ project_root: "/tmp", session_surface: surface }));
    vi.mocked(scanWorkspace).mockResolvedValue({
      root: "/tmp",
      projects: ["/tmp/example-repo"],
      conversations: [{ id: "codex-history", provider: "codex", title: "Continue the parser cleanup", cwd: "/tmp/example-repo", project_root: "/tmp/example-repo", updated_at: new Date().toISOString() }],
    });
    vi.mocked(createSession).mockResolvedValue({ id: "imported", title: "Continue the parser cleanup", prompt: "", agent: "codex", mode: expectedMode, repo_root: "/tmp/example-repo", worktree_path: "/tmp/worktree", branch: "openade/imported", base_branch: "HEAD", status: expectedMode === "tui" ? "running" : "completed", created_at: new Date().toISOString(), updated_at: new Date().toISOString() });
    const user = userEvent.setup();
    render(<AppShell />);
    const history = await screen.findAllByRole("button", { name: /Continue the parser cleanup/ });
    await user.click(history[0]);
    await waitFor(() => expect(createSession).toHaveBeenCalledWith(expect.objectContaining({ agent: "codex", mode: expectedMode, resume_id: "codex-history", repo_root: "/tmp/example-repo" })));
  });

  it("switches an indexed OpenADE session to the preferred surface when it opens", async () => {
    const indexed = { id: "existing-tui", title: "Continue the parser cleanup", prompt: "", agent: "codex", mode: "tui" as const, repo_root: "/tmp/example-repo", worktree_path: "/tmp/worktree", branch: "openade/existing", base_branch: "main", status: "completed" as const, created_at: new Date().toISOString(), updated_at: new Date().toISOString() };
    vi.mocked((await import("./api")).listSessions).mockResolvedValue([indexed]);
    vi.mocked(switchSessionSurface).mockResolvedValue({ ...indexed, mode: "chat", status: "completed" });
    localStorage.setItem("openade.preferences", JSON.stringify({ session_surface: "chat" }));

    const user = userEvent.setup();
    render(<AppShell />);
    const sessionButtons = await screen.findAllByRole("button", { name: /Continue the parser cleanup/ });
    await user.click(sessionButtons[0]);

    await waitFor(() => expect(switchSessionSurface).toHaveBeenCalledWith("existing-tui", "chat"));
  });
});

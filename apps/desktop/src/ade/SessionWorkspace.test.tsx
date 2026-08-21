import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Session } from "./api";
import { SessionWorkspace } from "./SessionWorkspace";
import { Preferences } from "./preferences";

vi.mock("./api", async (loadOriginal) => {
  const original = await loadOriginal<typeof import("./api")>();
  return {
    ...original,
    listAgentCommands: vi.fn().mockResolvedValue([]),
    streamURL: vi.fn().mockReturnValue("ws://openade.test/sessions/session-1/stream"),
  };
});

const session: Session = {
  id: "session-1",
  title: "Keep the agent workspace focused",
  prompt: "Improve the session workspace",
  agent: "codex",
  mode: "chat",
  repo_root: "/tmp/openade",
  worktree_path: "/tmp/openade-worktree",
  branch: "ade/session-inspector",
  base_branch: "main",
  status: "completed",
  created_at: "2026-08-21T04:00:00Z",
  updated_at: "2026-08-21T05:00:00Z",
};

const preferences: Preferences = {
  theme: "glass",
  default_agent: "codex",
  session_surface: "chat",
  activity_detail: "compact",
  project_root: "",
  project_organization: "project",
  project_sort: "priority",
};

afterEach(() => vi.unstubAllGlobals());

describe("Session workspace inspector", () => {
  it("keeps work surfaces in a right rail and toggles one inspector panel", async () => {
    class FakeWebSocket {
      onmessage: ((event: MessageEvent) => void) | null = null;
      close = vi.fn();
    }
    vi.stubGlobal("WebSocket", FakeWebSocket);
    HTMLElement.prototype.scrollTo = vi.fn();
    const user = userEvent.setup();

    render(<SessionWorkspace session={session} preferences={preferences} onBack={vi.fn()} onRefresh={vi.fn().mockResolvedValue(undefined)} />);

    expect(screen.queryByRole("navigation", { name: "Work surfaces" })).not.toBeInTheDocument();
    expect(screen.getByRole("complementary", { name: "Session tools" })).toBeInTheDocument();
    expect(screen.queryByRole("complementary", { name: "Pull request panel" })).not.toBeInTheDocument();

    const pullRequest = screen.getByRole("button", { name: "PR" });
    await user.click(pullRequest);

    expect(pullRequest).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("complementary", { name: "Pull request panel" })).toBeInTheDocument();
    expect(screen.getByText("GitHub delivery")).toBeInTheDocument();

    await user.click(pullRequest);
    expect(screen.queryByRole("complementary", { name: "Pull request panel" })).not.toBeInTheDocument();
  });
});

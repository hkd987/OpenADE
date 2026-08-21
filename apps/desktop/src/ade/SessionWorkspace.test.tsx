import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { enqueueMessage, Session } from "./api";
import { SessionWorkspace } from "./SessionWorkspace";
import { Preferences } from "./preferences";

const { daemonQueue } = vi.hoisted(() => ({ daemonQueue: [] as Array<{ id: string; session_id: string; text: string; status: "queued" | "dispatching"; priority: number; created_at: string; updated_at: string }> }));

vi.mock("./api", async (loadOriginal) => {
  const original = await loadOriginal<typeof import("./api")>();
  return {
    ...original,
    enqueueMessage: vi.fn().mockImplementation(async (sessionId: string, text: string) => {
      const message = { id: `queued-${daemonQueue.length + 1}`, session_id: sessionId, text, status: "queued" as const, priority: 0, created_at: "2026-08-21T05:00:00Z", updated_at: "2026-08-21T05:00:00Z" };
      daemonQueue.push(message);
      return message;
    }),
    listAgentCommands: vi.fn().mockResolvedValue([]),
    listMessageQueue: vi.fn().mockImplementation(async () => [...daemonQueue]),
    removeQueuedMessage: vi.fn().mockImplementation(async (_sessionId: string, messageId: string) => {
      const index = daemonQueue.findIndex((item) => item.id === messageId);
      if (index >= 0) daemonQueue.splice(index, 1);
    }),
    steerQueuedMessage: vi.fn().mockImplementation(async (_sessionId: string, messageId: string) => {
      const index = daemonQueue.findIndex((item) => item.id === messageId);
      if (index > 0) daemonQueue.unshift(...daemonQueue.splice(index, 1));
    }),
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

beforeEach(() => {
  daemonQueue.length = 0;
  const values = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
    clear: () => values.clear(),
  });
});

afterEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

function stubSessionRuntime() {
  class FakeWebSocket {
    onmessage: ((event: MessageEvent) => void) | null = null;
    onclose: (() => void) | null = null;
    close = vi.fn();
  }
  vi.stubGlobal("WebSocket", FakeWebSocket);
  HTMLElement.prototype.scrollTo = vi.fn();
}

describe("Session workspace inspector", () => {
  it("keeps work surfaces in a right rail and toggles one inspector panel", async () => {
    stubSessionRuntime();
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

  it("queues follow-up messages while the agent is working and supports steer, remove, and edit", async () => {
    stubSessionRuntime();
    const user = userEvent.setup();
    render(<SessionWorkspace session={{ ...session, status: "running" }} preferences={preferences} onBack={vi.fn()} onRefresh={vi.fn().mockResolvedValue(undefined)} />);

    const composer = screen.getByRole("textbox", { name: "" });
    await user.type(composer, "Run the focused tests{enter}");
    await user.type(composer, "Then update the pull request{enter}");

    const queue = screen.getByRole("region", { name: "Queued messages" });
    expect(within(queue).getByText("Step 1 / 3")).toBeInTheDocument();
    expect(within(queue).getByText("Run the focused tests")).toBeInTheDocument();
    expect(within(queue).getByText("Then update the pull request")).toBeInTheDocument();
    expect(enqueueMessage).toHaveBeenNthCalledWith(1, "session-1", "Run the focused tests");
    expect(enqueueMessage).toHaveBeenNthCalledWith(2, "session-1", "Then update the pull request");

    await user.click(within(queue).getAllByRole("button", { name: "Steer" })[1]);
    expect(within(queue).getAllByRole("article")[0]).toHaveTextContent("Then update the pull request");

    await user.click(within(queue).getByRole("button", { name: "Remove queued message 2" }));
    expect(within(queue).queryByText("Run the focused tests")).not.toBeInTheDocument();

    await user.click(within(queue).getByRole("button", { name: "Edit queued message 1" }));
    expect(composer).toHaveValue("Then update the pull request");
    expect(screen.queryByRole("region", { name: "Queued messages" })).not.toBeInTheDocument();
  });

  it("renders a daemon-owned message that is being dispatched", async () => {
    stubSessionRuntime();
    daemonQueue.push({ id: "queued-1", session_id: "session-1", text: "Continue with the next task", status: "dispatching", priority: 0, created_at: "2026-08-21T05:00:00Z", updated_at: "2026-08-21T05:00:00Z" });

    render(<SessionWorkspace session={session} preferences={preferences} onBack={vi.fn()} onRefresh={vi.fn().mockResolvedValue(undefined)} />);

    const queue = await screen.findByRole("region", { name: "Queued messages" });
    expect(within(queue).getByText("Continue with the next task")).toBeInTheDocument();
    expect(within(queue).getByRole("button", { name: "Sending" })).toBeDisabled();
  });
});

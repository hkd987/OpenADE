import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { enqueueMessage, listMessageQueue, Session } from "./api";
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
  vi.useRealTimers();
  localStorage.clear();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

function stubSessionRuntime() {
  const sockets: FakeWebSocket[] = [];
  class FakeWebSocket {
    onmessage: ((event: MessageEvent) => void) | null = null;
    onclose: (() => void) | null = null;
    close = vi.fn();
    constructor() { sockets.push(this); }
  }
  vi.stubGlobal("WebSocket", FakeWebSocket);
  HTMLElement.prototype.scrollTo = vi.fn();
  return sockets;
}

describe("Session workspace inspector", () => {
  it("does not open a chat stream for shell-only sessions", () => {
    const sockets = stubSessionRuntime();
    render(<SessionWorkspace session={{ ...session, agent: "shell" }} preferences={preferences} onBack={vi.fn()} onRefresh={vi.fn().mockResolvedValue(undefined)} />);
    expect(sockets).toHaveLength(0);
  });

  it("serializes queue polling while a daemon request is pending", async () => {
    vi.useFakeTimers();
    let resolveQueue!: (messages: Awaited<ReturnType<typeof listMessageQueue>>) => void;
    vi.mocked(listMessageQueue).mockImplementationOnce(() => new Promise((resolve) => {
      resolveQueue = resolve;
    }));
    stubSessionRuntime();
    const view = render(<SessionWorkspace session={{ ...session, status: "running" }} preferences={preferences} onBack={vi.fn()} onRefresh={vi.fn().mockResolvedValue(undefined)} />);

    expect(listMessageQueue).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2_400);
    });
    expect(listMessageQueue).toHaveBeenCalledTimes(1);
    view.unmount();
    await act(async () => {
      resolveQueue([]);
      await Promise.resolve();
      await vi.runAllTimersAsync();
    });
    expect(listMessageQueue).toHaveBeenCalledTimes(1);
  });

  it("clears a pending stream reconnect when the workspace closes", () => {
    vi.useFakeTimers();
    const sockets = stubSessionRuntime();
    const view = render(<SessionWorkspace session={{ ...session, status: "running" }} preferences={preferences} onBack={vi.fn()} onRefresh={vi.fn().mockResolvedValue(undefined)} />);
    expect(sockets).toHaveLength(1);
    act(() => sockets[0].onclose?.());
    expect(vi.getTimerCount()).toBeGreaterThan(0);

    view.unmount();
    expect(vi.getTimerCount()).toBe(0);
  });

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

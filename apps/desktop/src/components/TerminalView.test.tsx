import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SessionMeta } from "../api";
import { TerminalView } from "./TerminalView";

const { terminal, getScrollback, sendInput } = vi.hoisted(() => {
  const terminal = {
    open: vi.fn(),
    write: vi.fn(),
    reset: vi.fn(),
    dispose: vi.fn(),
    onDataHandler: undefined as undefined | ((data: string) => void),
    onData: vi.fn((cb: (data: string) => void) => {
      terminal.onDataHandler = cb;
      return { dispose: vi.fn() };
    }),
  };
  return {
    terminal,
    getScrollback: vi.fn(),
    sendInput: vi.fn(),
  };
});

vi.mock("@xterm/xterm", () => ({
  Terminal: vi.fn(function Terminal() {
    return terminal;
  }),
}));

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  getScrollback,
  sendInput,
}));

const session: SessionMeta = {
  id: "s-1",
  title: "task",
  harness: "claude-code",
  repo_root: "/repo",
  worktree_path: "/wt",
  state: "running",
  created_at: "2026-08-11T10:00:00Z",
  updated_at: "2026-08-11T10:00:00Z",
};

describe("TerminalView", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("streams scrollback deltas and resets when the buffer shrinks", async () => {
    getScrollback.mockResolvedValue({ scrollback: "hello" });
    render(<TerminalView session={session} />);
    expect(terminal.open).toHaveBeenCalled();

    // Initial poll writes the full buffer.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(terminal.write).toHaveBeenLastCalledWith("hello");

    // Next poll writes only the delta.
    getScrollback.mockResolvedValue({ scrollback: "hello world" });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(terminal.write).toHaveBeenLastCalledWith(" world");

    // Unchanged buffer: no extra write.
    const writes = terminal.write.mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(terminal.write.mock.calls.length).toBe(writes);

    // Shrunk buffer (daemon restart): reset and redraw.
    getScrollback.mockResolvedValue({ scrollback: "hi" });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(terminal.reset).toHaveBeenCalled();
    expect(terminal.write).toHaveBeenLastCalledWith("hi");
  });

  it("forwards keystrokes to the daemon and survives input errors", async () => {
    getScrollback.mockResolvedValue({ scrollback: "" });
    sendInput.mockRejectedValue(new Error("daemon gone"));
    render(<TerminalView session={session} />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    terminal.onDataHandler?.("y");
    expect(sendInput).toHaveBeenCalledWith("s-1", "y");
  });

  it("drops poll results that resolve after unmount", async () => {
    let resolvePoll: (v: { scrollback: string }) => void = () => {};
    getScrollback.mockImplementation(
      () => new Promise<{ scrollback: string }>((r) => (resolvePoll = r)),
    );
    const { unmount } = render(<TerminalView session={session} />);
    unmount();
    await act(async () => {
      resolvePoll({ scrollback: "late data" });
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(terminal.write).not.toHaveBeenCalledWith("late data");
  });

  it("ignores poll failures and disposes cleanly", async () => {
    getScrollback.mockRejectedValue(new Error("unreachable"));
    const { unmount } = render(<TerminalView session={session} />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    unmount();
    expect(terminal.dispose).toHaveBeenCalled();
  });
});

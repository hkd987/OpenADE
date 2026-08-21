import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Session } from "./api";
import { DirectTUIWorkspace } from "./Terminal";

const mocks = vi.hoisted(() => {
  const dimensions = { rows: 32, cols: 100 };
  const terminal = {
    rows: dimensions.rows,
    cols: dimensions.cols,
    loadAddon: vi.fn(),
    open: vi.fn(),
    write: vi.fn(),
    dispose: vi.fn(),
    onData: vi.fn(() => ({ dispose: vi.fn() })),
  };
  const fit = vi.fn(() => {
    terminal.rows = dimensions.rows;
    terminal.cols = dimensions.cols;
  });
  const inputDispose = vi.fn();
  return {
    dimensions,
    terminal,
    fit,
    inputDispose,
    resizeTerminal: vi.fn().mockResolvedValue(undefined),
  };
});

const resources = {
  sockets: [] as Array<{ onmessage: ((event: MessageEvent) => void) | null; close: ReturnType<typeof vi.fn> }>,
  observers: [] as Array<{ observe: ReturnType<typeof vi.fn>; disconnect: ReturnType<typeof vi.fn> }>,
};

vi.mock("@xterm/xterm", () => ({
  Terminal: vi.fn(function Terminal() {
    return mocks.terminal;
  }),
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: vi.fn(function FitAddon() {
    return { fit: mocks.fit };
  }),
}));

vi.mock("./api", async (loadOriginal) => ({
  ...(await loadOriginal<typeof import("./api")>()),
  resizeTerminal: mocks.resizeTerminal,
  streamURL: vi.fn().mockReturnValue("ws://openade.test/sessions/session-1/stream"),
}));

const session: Session = {
  id: "session-1",
  title: "Direct TUI",
  prompt: "Inspect the repository",
  agent: "claude",
  mode: "tui",
  repo_root: "/tmp/openade",
  worktree_path: "/tmp/openade-worktree",
  branch: "ade/direct-tui",
  base_branch: "main",
  status: "running",
  created_at: "2026-08-21T04:00:00Z",
  updated_at: "2026-08-21T05:00:00Z",
};

describe("Direct TUI sizing", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.dimensions.rows = 32;
    mocks.dimensions.cols = 100;
    mocks.terminal.rows = 32;
    mocks.terminal.cols = 100;
    mocks.terminal.onData.mockImplementation(() => ({ dispose: mocks.inputDispose }));
    resources.sockets.length = 0;
    resources.observers.length = 0;

    class FakeWebSocket {
      onmessage: ((event: MessageEvent) => void) | null = null;
      close = vi.fn();
      constructor() { resources.sockets.push(this); }
    }
    class FakeResizeObserver {
      observe = vi.fn();
      disconnect = vi.fn();
      constructor() { resources.observers.push(this); }
    }
    vi.stubGlobal("WebSocket", FakeWebSocket);
    vi.stubGlobal("ResizeObserver", FakeResizeObserver);
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    });
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    Object.defineProperties(HTMLElement.prototype, {
      clientWidth: { configurable: true, get: () => 1000 },
      clientHeight: { configurable: true, get: () => 700 },
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("refits the terminal and resizes its daemon PTY when the window changes size", async () => {
    render(<DirectTUIWorkspace session={session} onRefresh={vi.fn().mockResolvedValue(undefined)} />);

    expect(mocks.fit).toHaveBeenCalledTimes(1);
    expect(mocks.resizeTerminal).toHaveBeenLastCalledWith("session-1", 32, 100);

    mocks.dimensions.rows = 48;
    mocks.dimensions.cols = 156;
    await act(async () => {
      window.dispatchEvent(new Event("resize"));
    });

    expect(mocks.fit).toHaveBeenCalledTimes(2);
    expect(mocks.resizeTerminal).toHaveBeenLastCalledWith("session-1", 48, 156);
  });

  it("releases every terminal resource when the TUI closes", () => {
    const view = render(<DirectTUIWorkspace session={session} onRefresh={vi.fn().mockResolvedValue(undefined)} />);
    const socket = resources.sockets[0];
    const observer = resources.observers[0];
    expect(socket.onmessage).toBeTypeOf("function");

    view.unmount();

    expect(socket.onmessage).toBeNull();
    expect(socket.close).toHaveBeenCalledOnce();
    expect(observer.disconnect).toHaveBeenCalledOnce();
    expect(mocks.inputDispose).toHaveBeenCalledOnce();
    expect(mocks.terminal.dispose).toHaveBeenCalledOnce();
    expect(cancelAnimationFrame).toHaveBeenCalled();
  });
});

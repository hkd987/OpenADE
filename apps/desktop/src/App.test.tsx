import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { SessionMeta } from "./api";

const { listSessions } = vi.hoisted(() => ({ listSessions: vi.fn() }));

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  listSessions,
}));

vi.mock("./components/NewSessionForm", () => ({
  NewSessionForm: ({
    onCreated,
    onClose,
  }: {
    onCreated: (s: SessionMeta) => void;
    onClose: () => void;
  }) => (
    <div data-testid="form-stub">
      <button
        data-testid="stub-create"
        onClick={() =>
          onCreated({
            id: "new-1",
            title: "created",
            harness: "claude-code",
            repo_root: "/repo",
            state: "running",
            created_at: "",
            updated_at: "",
          })
        }
      >
        create
      </button>
      <button data-testid="stub-close" onClick={onClose}>
        close
      </button>
    </div>
  ),
}));

vi.mock("./components/SessionDetail", () => ({
  SessionDetail: ({
    session,
    onChanged,
  }: {
    session: SessionMeta;
    onChanged: (selectId?: string) => void;
  }) => (
    <div data-testid="detail-stub">
      {session.id}
      <button data-testid="stub-select-other" onClick={() => onChanged("s-2")}>
        select other
      </button>
      <button data-testid="stub-refresh" onClick={() => onChanged()}>
        refresh
      </button>
    </div>
  ),
}));

const running: SessionMeta = {
  id: "s-1",
  title: "task one",
  harness: "claude-code",
  repo_root: "/repo",
  state: "running",
  created_at: "2026-08-11T10:00:00Z",
  updated_at: "2026-08-11T10:00:00Z",
};

describe("App", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows the empty state, then sessions as they appear", async () => {
    listSessions.mockResolvedValue({ sessions: [] });
    render(<App />);
    expect(await screen.findByTestId("empty-grid")).toBeInTheDocument();

    listSessions.mockResolvedValue({ sessions: [running] });
    // The poll loop refreshes the grid.
    await act(async () => {
      await new Promise((r) => setTimeout(r, 2100));
    });
    expect(screen.getByText("task one")).toBeInTheDocument();
  }, 10_000);

  it("shows a daemon error banner when unreachable", async () => {
    listSessions.mockRejectedValue(new Error("Failed to fetch"));
    render(<App />);
    expect(await screen.findByTestId("daemon-error")).toHaveTextContent(
      "Failed to fetch",
    );
  });

  it("selects a session card and renders its detail", async () => {
    const other: SessionMeta = { ...running, id: "s-2", title: "task two" };
    listSessions.mockResolvedValue({ sessions: [running, other] });
    render(<App />);
    await userEvent.click(await screen.findByText("task one"));
    expect(screen.getByTestId("detail-stub")).toHaveTextContent("s-1");

    // Detail actions can re-select (handoff) or just trigger a refresh.
    await userEvent.click(screen.getByTestId("stub-select-other"));
    expect(screen.getByTestId("detail-stub")).toHaveTextContent("s-2");
    await userEvent.click(screen.getByTestId("stub-refresh"));
    expect(screen.getByTestId("detail-stub")).toHaveTextContent("s-2");
  });

  it("opens the launch form and selects the created session", async () => {
    listSessions.mockResolvedValue({ sessions: [running] });
    render(<App />);
    await userEvent.click(screen.getByTestId("toggle-new-session"));
    expect(screen.getByTestId("form-stub")).toBeInTheDocument();

    await userEvent.click(screen.getByTestId("stub-create"));
    expect(screen.queryByTestId("form-stub")).not.toBeInTheDocument();

    // Reopen and close through the form's cancel path.
    await userEvent.click(screen.getByTestId("toggle-new-session"));
    await userEvent.click(screen.getByTestId("stub-close"));
    expect(screen.queryByTestId("form-stub")).not.toBeInTheDocument();
  });
});

import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { SessionMeta } from "./api";

const { listSessions, getConfig } = vi.hoisted(() => ({
  listSessions: vi.fn(),
  getConfig: vi.fn(),
}));

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  listSessions,
  getConfig,
}));

vi.mock("./components/Onboarding", () => ({
  Onboarding: ({ onDone }: { onDone: () => void }) => (
    <div data-testid="onboarding-stub">
      <button data-testid="stub-onboard-done" onClick={onDone} />
    </div>
  ),
}));

vi.mock("./components/NewSessionForm", () => ({
  NewSessionForm: ({
    onCreated,
    onClose,
    initialRepo,
  }: {
    onCreated: (s: SessionMeta) => void;
    onClose: () => void;
    initialRepo?: string;
  }) => (
    <div data-testid="form-stub">
      {initialRepo !== undefined && <span>repo:{initialRepo}</span>}
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

const onboardedConfig = {
  onboarded: true,
  backstage_base_url: null,
  backstage_token_set: false,
  memory_repo: null,
  memory_sources: ["github"],
  gh_found: true,
  gh_authenticated: true,
};

describe("App", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getConfig.mockResolvedValue(onboardedConfig);
  });

  it("shows first-run onboarding until it completes", async () => {
    getConfig.mockResolvedValue({ ...onboardedConfig, onboarded: false });
    listSessions.mockResolvedValue({ sessions: [] });
    render(<App />);
    expect(await screen.findByTestId("onboarding-stub")).toBeInTheDocument();

    // Completing onboarding re-fetches config and dismisses the overlay.
    getConfig.mockResolvedValue(onboardedConfig);
    await userEvent.click(screen.getByTestId("stub-onboard-done"));
    await act(async () => {});
    expect(screen.queryByTestId("onboarding-stub")).toBeNull();
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
    // Config is equally unreachable — onboarding must not pop up over the
    // error banner.
    getConfig.mockRejectedValue(new Error("Failed to fetch"));
    render(<App />);
    expect(await screen.findByTestId("daemon-error")).toHaveTextContent(
      "Failed to fetch",
    );
    expect(screen.queryByTestId("onboarding-stub")).toBeNull();
  });

  it("groups the grid by project", async () => {
    const otherRepo: SessionMeta = {
      ...running,
      id: "s-9",
      title: "other repo task",
      repo_root: "/repos/ledger",
    };
    listSessions.mockResolvedValue({ sessions: [running, otherRepo] });
    render(<App />);
    const headers = await screen.findAllByTestId("project-group-header");
    expect(headers.map((h) => h.textContent?.replace(/[▸▾]/g, ""))).toEqual([
      "repo",
      "ledger",
    ]);
  });

  it("collapses and expands a project group", async () => {
    listSessions.mockResolvedValue({ sessions: [running] });
    render(<App />);
    expect(await screen.findByText("task one")).toBeInTheDocument();

    const header = screen.getByTestId("project-group-header");
    await userEvent.click(header);
    expect(screen.queryByText("task one")).toBeNull();
    expect(header).toHaveAttribute("aria-expanded", "false");

    await userEvent.click(header);
    expect(screen.getByText("task one")).toBeInTheDocument();
  });

  it("launches into a project from its + button", async () => {
    listSessions.mockResolvedValue({ sessions: [running] });
    render(<App />);
    await userEvent.click(await screen.findByTestId("project-add"));
    // The form opens pre-filled with that project's repository.
    expect(screen.getByTestId("form-stub")).toHaveTextContent("repo:/repo");
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

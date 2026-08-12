import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { WorkspaceSession } from "../api";
import { TeamView } from "./TeamView";

const { listWorkspaceSessions, getWorkspaceSession, pickupSession } =
  vi.hoisted(() => ({
    listWorkspaceSessions: vi.fn(),
    getWorkspaceSession: vi.fn(),
    pickupSession: vi.fn(),
  }));

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  listWorkspaceSessions,
  getWorkspaceSession,
  pickupSession,
}));

const shared: WorkspaceSession = {
  id: 7,
  title: "add retries",
  harness: "claude-code",
  entity_ref: "repo:acme/payments",
  summary: "Added retry logic to the payment poller.",
  shared_by: "casey",
  uploaded_at: "2026-08-11T10:00:00Z",
};

describe("TeamView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listWorkspaceSessions.mockResolvedValue({ sessions: [shared] });
    getWorkspaceSession.mockResolvedValue({
      session: shared,
      markdown: "# Session: add retries",
      events: [
        { kind: "prompt", payload: { text: "do it" } },
        { kind: "state" },
      ],
    });
  });

  it("explains how to turn multiplayer on when unconfigured", () => {
    render(<TeamView configured={false} repos={[]} onPickedUp={vi.fn()} />);
    expect(screen.getByTestId("team-unconfigured")).toHaveTextContent(
      "openade-server",
    );
    expect(listWorkspaceSessions).not.toHaveBeenCalled();
  });

  it("lists shared sessions with author, harness, and entity", async () => {
    render(<TeamView configured repos={[]} onPickedUp={vi.fn()} />);
    const row = await screen.findByTestId("team-row");
    expect(row).toHaveTextContent("add retries");
    expect(row).toHaveTextContent("casey");
    expect(row).toHaveTextContent("Claude Code");
    expect(row).toHaveTextContent("acme/payments");
    expect(row).toHaveTextContent("Added retry logic");
  });

  it("shows the empty state when nothing has been shared", async () => {
    listWorkspaceSessions.mockResolvedValue({ sessions: [] });
    render(<TeamView configured repos={[]} onPickedUp={vi.fn()} />);
    expect(await screen.findByTestId("team-empty")).toHaveTextContent(
      "No shared sessions yet",
    );
  });

  it("surfaces list errors (bad token, server down)", async () => {
    listWorkspaceSessions.mockRejectedValue(
      new Error("workspace server rejected the token"),
    );
    render(<TeamView configured repos={[]} onPickedUp={vi.fn()} />);
    expect(await screen.findByTestId("team-error")).toHaveTextContent(
      "rejected the token",
    );
  });

  it("opens the read-only record with artifact and transcript, and goes back", async () => {
    render(<TeamView configured repos={[]} onPickedUp={vi.fn()} />);
    await userEvent.click(await screen.findByTestId("team-row"));

    const detail = await screen.findByTestId("team-detail");
    expect(detail).toHaveTextContent("add retries");
    expect(screen.getByTestId("team-markdown")).toHaveTextContent(
      "# Session: add retries",
    );
    // Events render with their kind; payload text only when present.
    const transcript = screen.getByTestId("team-transcript");
    expect(transcript).toHaveTextContent("prompt");
    expect(transcript).toHaveTextContent("do it");
    expect(transcript).toHaveTextContent("state");

    await userEvent.click(screen.getByTestId("team-back"));
    expect(screen.queryByTestId("team-detail")).toBeNull();
    expect(screen.getByTestId("team-row")).toBeInTheDocument();
  });

  it("surfaces detail-load errors", async () => {
    getWorkspaceSession.mockRejectedValue(new Error("detail boom"));
    render(<TeamView configured repos={[]} onPickedUp={vi.fn()} />);
    await userEvent.click(await screen.findByTestId("team-row"));
    expect(await screen.findByTestId("team-error")).toHaveTextContent(
      "detail boom",
    );
  });

  it("picks a session up into the chosen harness and repo", async () => {
    pickupSession.mockResolvedValue({ id: "local-1", state: "running" });
    const onPickedUp = vi.fn();
    render(
      <TeamView configured repos={["/repos/payments"]} onPickedUp={onPickedUp} />,
    );
    await userEvent.click(await screen.findByTestId("team-row"));

    // The repo prefills from the known projects; any harness is choosable.
    expect(screen.getByTestId("pickup-repo")).toHaveValue("/repos/payments");
    await userEvent.selectOptions(
      screen.getByTestId("pickup-harness"),
      "gemini-cli",
    );
    await userEvent.click(screen.getByTestId("pickup-button"));
    await waitFor(() =>
      expect(onPickedUp).toHaveBeenCalledWith({
        id: "local-1",
        state: "running",
      }),
    );
    expect(pickupSession).toHaveBeenCalledWith({
      workspace_session_id: 7,
      harness: "gemini-cli",
      repo_root: "/repos/payments",
    });
  });

  it("requires a repo and surfaces pickup failures", async () => {
    render(<TeamView configured repos={[]} onPickedUp={vi.fn()} />);
    await userEvent.click(await screen.findByTestId("team-row"));

    // No repo → the button stays disabled.
    expect(screen.getByTestId("pickup-repo")).toHaveValue("");
    expect(screen.getByTestId("pickup-button")).toBeDisabled();

    pickupSession.mockRejectedValue(new Error("repository not found"));
    await userEvent.type(screen.getByTestId("pickup-repo"), "/nope");
    await userEvent.click(screen.getByTestId("pickup-button"));
    expect(await screen.findByTestId("team-error")).toHaveTextContent(
      "repository not found",
    );
    // The button re-enables after the failure.
    expect(screen.getByTestId("pickup-button")).toBeEnabled();
  });
});

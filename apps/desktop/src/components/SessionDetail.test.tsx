import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionMeta } from "../api";
import { SessionDetail } from "./SessionDetail";

const { getDiff, getFiles, handoffSession, killSession, publishArtifact } =
  vi.hoisted(() => ({
    getDiff: vi.fn(),
    getFiles: vi.fn(),
    handoffSession: vi.fn(),
    killSession: vi.fn(),
    publishArtifact: vi.fn(),
  }));

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  getDiff,
  getFiles,
  handoffSession,
  killSession,
  publishArtifact,
}));

vi.mock("./TerminalView", () => ({
  TerminalView: () => <div data-testid="terminal-stub" />,
}));

const session: SessionMeta = {
  id: "s-1",
  title: "add retries",
  harness: "claude-code",
  repo_root: "/repo",
  worktree_path: "/wt",
  state: "running",
  created_at: "2026-08-11T10:00:00Z",
  updated_at: "2026-08-11T10:00:00Z",
};

describe("SessionDetail", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getDiff.mockResolvedValue({ diff: "+added line" });
    getFiles.mockResolvedValue({ files: ["README.md", "src/lib.rs"] });
  });

  it("switches between terminal, diff, and files tabs", async () => {
    render(<SessionDetail session={session} onChanged={() => {}} />);
    expect(screen.getByTestId("terminal-stub")).toBeInTheDocument();

    await userEvent.click(screen.getByTestId("tab-diff"));
    expect(await screen.findByTestId("diff-view")).toHaveTextContent("+added line");

    await userEvent.click(screen.getByTestId("tab-files"));
    const files = await screen.findByTestId("file-list");
    expect(files).toHaveTextContent("README.md");
    expect(files).toHaveTextContent("src/lib.rs");

    await userEvent.click(screen.getByTestId("tab-terminal"));
    expect(screen.getByTestId("terminal-stub")).toBeInTheDocument();
  });

  it("shows the empty-diff placeholder", async () => {
    getDiff.mockResolvedValue({ diff: "" });
    render(<SessionDetail session={session} onChanged={() => {}} />);
    await userEvent.click(screen.getByTestId("tab-diff"));
    expect(await screen.findByTestId("diff-view")).toHaveTextContent(
      "No changes against the task's base commit yet.",
    );
  });

  it("publishes an artifact and shows the review branch", async () => {
    publishArtifact.mockResolvedValue({
      branch: "openade/knowledge-x",
      file: "docs/openade/sessions/x.md",
      summary: "s",
      markdown: "# S",
    });
    const onChanged = vi.fn();
    render(<SessionDetail session={session} onChanged={onChanged} />);
    await userEvent.click(screen.getByTestId("artifact-button"));
    const banner = await screen.findByTestId("artifact-banner");
    expect(banner).toHaveTextContent("openade/knowledge-x");
    // No shared memory repo configured — no team-memory link.
    expect(screen.queryByTestId("shared-memory-link")).toBeNull();
    expect(onChanged).toHaveBeenCalled();
  });

  it("links the shared team memory copy when the artifact was pushed there", async () => {
    publishArtifact.mockResolvedValue({
      branch: "openade/knowledge-x",
      file: "docs/openade/sessions/x.md",
      summary: "s",
      markdown: "# S",
      shared_repo: "acme/team-memory",
      shared_path: "sessions/x.md",
    });
    render(<SessionDetail session={session} onChanged={vi.fn()} />);
    await userEvent.click(screen.getByTestId("artifact-button"));
    const link = await screen.findByTestId("shared-memory-link");
    expect(link).toHaveTextContent("acme/team-memory");
    expect(link).toHaveAttribute(
      "href",
      "https://github.com/acme/team-memory/blob/HEAD/sessions/x.md",
    );
  });

  it("hands off to the selected harness and selects the new session", async () => {
    handoffSession.mockResolvedValue({ ...session, id: "s-2", harness: "gemini-cli" });
    const onChanged = vi.fn();
    render(<SessionDetail session={session} onChanged={onChanged} />);
    await userEvent.selectOptions(screen.getByTestId("handoff-target"), "gemini-cli");
    await userEvent.click(screen.getByTestId("handoff-button"));
    await waitFor(() => expect(onChanged).toHaveBeenCalledWith("s-2"));
    expect(handoffSession).toHaveBeenCalledWith("s-1", "gemini-cli");
  });

  it("kills the session and surfaces action errors", async () => {
    killSession.mockResolvedValue({ ...session, state: "failed" });
    const onChanged = vi.fn();
    const { rerender } = render(
      <SessionDetail session={session} onChanged={onChanged} />,
    );
    await userEvent.click(screen.getByTestId("kill-button"));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());

    // Errors from actions render as an alert instead of vanishing.
    publishArtifact.mockRejectedValue(new Error("worktree is dirty"));
    rerender(<SessionDetail session={session} onChanged={onChanged} />);
    await userEvent.click(screen.getByTestId("artifact-button"));
    expect(await screen.findByRole("alert")).toHaveTextContent("worktree is dirty");
  });

  it("surfaces tab-load errors", async () => {
    getDiff.mockRejectedValue(new Error("boom"));
    render(<SessionDetail session={session} onChanged={() => {}} />);
    await userEvent.click(screen.getByTestId("tab-diff"));
    expect(await screen.findByRole("alert")).toHaveTextContent("boom");

    getFiles.mockRejectedValue(new Error("files boom"));
    await userEvent.click(screen.getByTestId("tab-files"));
    expect(await screen.findByRole("alert")).toHaveTextContent("files boom");
  });
});

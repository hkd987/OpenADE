import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionMeta } from "../api";
import { SessionDetail } from "./SessionDetail";

const { getDiff, getFiles,
  getFileContent,
  getSkills, handoffSession, killSession, publishArtifact, shareSession,
  recordInboxOutcome } =
  vi.hoisted(() => ({
    getDiff: vi.fn(),
    getFiles: vi.fn(),
    getFileContent: vi.fn(),
    getSkills: vi.fn(),
    handoffSession: vi.fn(),
    killSession: vi.fn(),
    publishArtifact: vi.fn(),
    shareSession: vi.fn(),
    recordInboxOutcome: vi.fn(),
  }));

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  getDiff,
  getFiles,
  getFileContent,
  getSkills,
  handoffSession,
  killSession,
  publishArtifact,
  shareSession,
  recordInboxOutcome,
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
    // The header shows how long the session has been going.
    expect(screen.getByTestId("session-runtime")).toHaveTextContent("started");
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

  it("hides Share until a workspace is configured, then shares with a banner", async () => {
    shareSession.mockResolvedValue({
      id: 7,
      title: "add retries",
      harness: "claude-code",
      summary: "s",
      shared_by: "casey",
      uploaded_at: "2026-08-11T10:00:00Z",
    });
    const { rerender } = render(
      <SessionDetail session={session} onChanged={vi.fn()} />,
    );
    expect(screen.queryByTestId("share-button")).toBeNull();

    rerender(
      <SessionDetail
        session={session}
        onChanged={vi.fn()}
        workspaceConfigured
      />,
    );
    await userEvent.click(screen.getByTestId("share-button"));
    const banner = await screen.findByTestId("share-banner");
    expect(banner).toHaveTextContent("#7");
    expect(banner).toHaveTextContent("Team view");
    expect(shareSession).toHaveBeenCalledWith("s-1");

    // Share failures (server down, bad token) surface as the alert.
    shareSession.mockRejectedValue(
      new Error("workspace server rejected the token"),
    );
    await userEvent.click(screen.getByTestId("share-button"));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "rejected the token",
    );
  });

  it("triage sessions record their outcome automatically on completion", async () => {
    recordInboxOutcome.mockResolvedValue({ recorded: true, kind: "merged" });
    const triage: SessionMeta = {
      ...session,
      inbox_item_id: 7,
      state: "completed",
    };
    render(<SessionDetail session={triage} onChanged={vi.fn()} />);
    const banner = await screen.findByTestId("outcome-banner");
    expect(banner).toHaveTextContent("outcome recorded: merged");
    // One automatic attempt per session — idempotent by design.
    expect(recordInboxOutcome).toHaveBeenCalledTimes(1);

    // The manual button re-checks on demand (e.g. after the PR merges).
    recordInboxOutcome.mockResolvedValue({
      recorded: false,
      note: "no decided PR for the session branch yet",
    });
    await userEvent.click(screen.getByTestId("outcome-button"));
    expect(await screen.findByTestId("outcome-banner")).toHaveTextContent(
      "no decided PR",
    );
  });

  it("running or plain sessions do not auto-record outcomes", async () => {
    const triage: SessionMeta = { ...session, inbox_item_id: 7 };
    const { rerender } = render(
      <SessionDetail session={triage} onChanged={vi.fn()} />,
    );
    expect(recordInboxOutcome).not.toHaveBeenCalled();
    expect(screen.queryByTestId("outcome-button")).toBeInTheDocument();

    // A session with no inbox link shows no outcome affordances at all,
    // and auto-record failures surface in the banner.
    rerender(<SessionDetail session={session} onChanged={vi.fn()} />);
    expect(screen.queryByTestId("outcome-button")).toBeNull();
    recordInboxOutcome.mockRejectedValue(new Error("gh exploded"));
    rerender(
      <SessionDetail
        session={{ ...session, id: "s-2", inbox_item_id: 7, state: "failed" }}
        onChanged={vi.fn()}
      />,
    );
    expect(await screen.findByTestId("outcome-banner")).toHaveTextContent(
      "gh exploded",
    );
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

  it("files tab opens a read-only viewer; rules and skills tabs list their sources", async () => {
    getFiles.mockResolvedValue({ files: ["README.md", "CLAUDE.md"] });
    getFileContent.mockResolvedValue({ path: "CLAUDE.md", content: "# Rules body" });
    getSkills.mockResolvedValue({
      skills: [{ name: "release", path: ".claude/skills/release/SKILL.md", description: "Cut a release." }],
    });
    render(<SessionDetail session={session} onChanged={vi.fn()} />);

    await userEvent.click(screen.getByTestId("tab-files"));
    await userEvent.click(await screen.findByText("README.md"));
    expect(await screen.findByTestId("file-viewer")).toHaveTextContent("# Rules body");
    await userEvent.click(screen.getByTestId("file-viewer-back"));
    expect(screen.queryByTestId("file-viewer")).toBeNull();

    // Rules tab filters to instruction files only, and opens the viewer.
    await userEvent.click(screen.getByTestId("tab-rules"));
    const rules = await screen.findByTestId("rules-list");
    expect(rules).toHaveTextContent("CLAUDE.md");
    expect(rules).not.toHaveTextContent("README.md");
    await userEvent.click(screen.getByText("CLAUDE.md"));
    expect(await screen.findByTestId("file-viewer")).toBeInTheDocument();
    await userEvent.click(screen.getByTestId("file-viewer-back"));

    // Skills tab lists discovered skills; clicking opens the skill file.
    await userEvent.click(screen.getByTestId("tab-skills"));
    const skills = await screen.findByTestId("skills-list");
    expect(skills).toHaveTextContent("release");
    expect(skills).toHaveTextContent("Cut a release.");
    await userEvent.click(screen.getByText("release"));
    expect(await screen.findByTestId("file-viewer")).toBeInTheDocument();
  });

  it("rules and skills tabs explain themselves when empty", async () => {
    getFiles.mockResolvedValue({ files: ["README.md"] });
    getSkills.mockResolvedValue({ skills: [] });
    render(<SessionDetail session={session} onChanged={vi.fn()} />);
    await userEvent.click(screen.getByTestId("tab-rules"));
    expect(await screen.findByTestId("rules-list")).toHaveTextContent("No rules files");
    await userEvent.click(screen.getByTestId("tab-skills"));
    expect(await screen.findByTestId("skills-list")).toHaveTextContent("No skills discovered");
  });

  it("surfaces skills and file-content errors", async () => {
    getSkills.mockRejectedValue(new Error("skills boom"));
    render(<SessionDetail session={session} onChanged={vi.fn()} />);
    await userEvent.click(screen.getByTestId("tab-skills"));
    expect(await screen.findByRole("alert")).toHaveTextContent("skills boom");

    getFileContent.mockRejectedValue(new Error("read boom"));
    await userEvent.click(screen.getByTestId("tab-files"));
    await userEvent.click(await screen.findByText("README.md"));
    expect(await screen.findByRole("alert")).toHaveTextContent("read boom");
  });
});

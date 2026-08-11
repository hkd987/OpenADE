import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NewSessionForm } from "./NewSessionForm";

const { createSession, listProjects } = vi.hoisted(() => ({
  createSession: vi.fn(),
  listProjects: vi.fn(),
}));

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  createSession,
  listProjects,
}));

describe("NewSessionForm", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listProjects.mockResolvedValue({ projects: ["/repos/alpha"] });
  });

  it("prefills the repository from known projects", async () => {
    render(<NewSessionForm onCreated={() => {}} onClose={() => {}} />);
    await waitFor(() =>
      expect(screen.getByTestId("ns-repo")).toHaveValue("/repos/alpha"),
    );
  });

  it("submits a launch request and reports the created session", async () => {
    const onCreated = vi.fn();
    createSession.mockResolvedValue({ id: "new-1", state: "running" });
    render(<NewSessionForm onCreated={onCreated} onClose={() => {}} />);

    await userEvent.type(screen.getByTestId("ns-title"), "fix the bug");
    await userEvent.selectOptions(screen.getByTestId("ns-harness"), "gemini-cli");
    await userEvent.type(
      screen.getByTestId("ns-entity"),
      "component:default/ledger",
    );
    await userEvent.click(screen.getByTestId("ns-submit"));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith({ id: "new-1", state: "running" }));
    expect(createSession).toHaveBeenCalledWith({
      title: "fix the bug",
      harness: "gemini-cli",
      repo_root: "/repos/alpha",
      entity_ref: "component:default/ledger",
      prompt: undefined,
    });
  });

  it("tolerates a project-list failure and accepts manual input", async () => {
    listProjects.mockRejectedValue(new Error("no daemon"));
    createSession.mockResolvedValue({ id: "manual-1", state: "running" });
    const onCreated = vi.fn();
    render(<NewSessionForm onCreated={onCreated} onClose={() => {}} />);

    await userEvent.type(screen.getByTestId("ns-title"), "manual task");
    await userEvent.type(screen.getByTestId("ns-repo"), "/typed/repo");
    await userEvent.type(screen.getByTestId("ns-prompt"), "do it carefully");
    await userEvent.click(screen.getByTestId("ns-submit"));

    await waitFor(() => expect(onCreated).toHaveBeenCalled());
    expect(createSession).toHaveBeenCalledWith({
      title: "manual task",
      harness: "claude-code",
      repo_root: "/typed/repo",
      entity_ref: undefined,
      prompt: "do it carefully",
    });
  });

  it("shows daemon errors instead of closing", async () => {
    const onCreated = vi.fn();
    createSession.mockRejectedValue(new Error("`/nope` is not a git repository"));
    render(<NewSessionForm onCreated={onCreated} onClose={() => {}} />);

    await userEvent.type(screen.getByTestId("ns-title"), "t");
    await userEvent.click(screen.getByTestId("ns-submit"));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "not a git repository",
    );
    expect(onCreated).not.toHaveBeenCalled();
  });
});

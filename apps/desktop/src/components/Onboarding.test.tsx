import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DaemonConfig } from "../api";
import { Onboarding } from "./Onboarding";

const { putConfig } = vi.hoisted(() => ({ putConfig: vi.fn() }));

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  putConfig,
}));

const base: DaemonConfig = {
  onboarded: false,
  backstage_base_url: null,
  backstage_token_set: false,
  memory_repo: null,
  memory_sources: ["github"],
  gh_found: true,
  gh_authenticated: true,
  server_url: null,
  server_token_set: false,
  server_workspace: null,
  workspace_configured: false,
};

describe("Onboarding", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("reports a ready GitHub CLI", () => {
    render(<Onboarding config={base} onDone={() => {}} />);
    expect(screen.getByTestId("ob-gh-status")).toHaveTextContent(
      "GitHub memory is ready",
    );
  });

  it("tells a signed-out user exactly how to fix gh", () => {
    render(
      <Onboarding
        config={{ ...base, gh_authenticated: false }}
        onDone={() => {}}
      />,
    );
    expect(screen.getByTestId("ob-gh-status")).toHaveTextContent(
      "not signed in",
    );
    expect(screen.getByTestId("ob-gh-status")).toHaveTextContent(
      "gh auth login",
    );
  });

  it("points a user without gh at the install docs", () => {
    render(
      <Onboarding
        config={{ ...base, gh_found: false, gh_authenticated: null }}
        onDone={() => {}}
      />,
    );
    const status = screen.getByTestId("ob-gh-status");
    expect(status).toHaveTextContent("GitHub CLI not found");
    expect(screen.getByRole("link", { name: "cli.github.com" })).toHaveAttribute(
      "href",
      "https://cli.github.com",
    );
  });

  it("saves the collected settings and completes", async () => {
    putConfig.mockResolvedValue({ ...base, onboarded: true });
    const onDone = vi.fn();
    render(<Onboarding config={base} onDone={onDone} />);
    await userEvent.type(
      screen.getByTestId("ob-backstage-url"),
      "https://backstage.example.com",
    );
    await userEvent.type(screen.getByTestId("ob-backstage-token"), "tok");
    await userEvent.type(screen.getByTestId("ob-memory-repo"), "acme/memory");
    await userEvent.type(
      screen.getByTestId("ob-server-url"),
      "http://hub:7500",
    );
    await userEvent.type(screen.getByTestId("ob-server-token"), "oadk_x");
    await userEvent.type(screen.getByTestId("ob-server-workspace"), "2");
    await userEvent.click(screen.getByTestId("ob-save"));
    expect(putConfig).toHaveBeenCalledWith({
      backstage_base_url: "https://backstage.example.com",
      backstage_token: "tok",
      memory_repo: "acme/memory",
      onboarded: true,
      server_url: "http://hub:7500",
      server_token: "oadk_x",
      server_workspace: 2,
    });
    expect(onDone).toHaveBeenCalled();
  });

  it("skip records onboarding without any settings", async () => {
    putConfig.mockResolvedValue({ ...base, onboarded: true });
    const onDone = vi.fn();
    render(<Onboarding config={base} onDone={onDone} />);
    await userEvent.click(screen.getByTestId("ob-skip"));
    expect(putConfig).toHaveBeenCalledWith({ onboarded: true });
    expect(onDone).toHaveBeenCalled();
  });

  it("settings mode prefills, keeps an untouched token, and cancels", async () => {
    putConfig.mockResolvedValue({ ...base, onboarded: true });
    const onDone = vi.fn();
    const onClose = vi.fn();
    render(
      <Onboarding
        mode="settings"
        config={{
          ...base,
          onboarded: true,
          backstage_base_url: "https://backstage.example.com",
          backstage_token_set: true,
          memory_repo: "acme/team-memory",
          server_url: "http://hub:7500",
          server_token_set: true,
          server_workspace: 3,
          workspace_configured: true,
        }}
        onDone={onDone}
        onClose={onClose}
      />,
    );
    // Prefilled from the live config; the token is never echoed back.
    expect(screen.getByTestId("ob-backstage-url")).toHaveValue(
      "https://backstage.example.com",
    );
    expect(screen.getByTestId("ob-memory-repo")).toHaveValue(
      "acme/team-memory",
    );
    expect(screen.getByTestId("ob-backstage-token")).toHaveAttribute(
      "placeholder",
      "(unchanged — type to replace)",
    );
    // Multiplayer fields prefill too; the member token is never echoed.
    expect(screen.getByTestId("ob-server-url")).toHaveValue(
      "http://hub:7500",
    );
    expect(screen.getByTestId("ob-server-workspace")).toHaveValue("3");
    expect(screen.getByTestId("ob-server-token")).toHaveAttribute(
      "placeholder",
      "(unchanged — type to replace)",
    );
    expect(screen.getByTestId("ob-sources")).toHaveTextContent("github");

    // Saving without touching the tokens omits them — the daemon keeps the
    // stored ones.
    await userEvent.click(screen.getByTestId("ob-save"));
    expect(putConfig).toHaveBeenCalledWith({
      backstage_base_url: "https://backstage.example.com",
      backstage_token: undefined,
      memory_repo: "acme/team-memory",
      onboarded: true,
      server_url: "http://hub:7500",
      server_token: undefined,
      server_workspace: 3,
    });
    expect(onDone).toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("settings mode cancel closes without saving", async () => {
    const onClose = vi.fn();
    render(
      <Onboarding
        mode="settings"
        config={{ ...base, onboarded: true }}
        onDone={vi.fn()}
        onClose={onClose}
      />,
    );
    await userEvent.click(screen.getByTestId("ob-cancel"));
    expect(onClose).toHaveBeenCalled();
    expect(putConfig).not.toHaveBeenCalled();
  });

  it("settings mode sends a typed token", async () => {
    putConfig.mockResolvedValue({ ...base, onboarded: true });
    render(
      <Onboarding
        mode="settings"
        config={{ ...base, onboarded: true }}
        onDone={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    await userEvent.type(screen.getByTestId("ob-backstage-token"), "new-tok");
    await userEvent.type(screen.getByTestId("ob-server-token"), "oadk_new");
    await userEvent.click(screen.getByTestId("ob-save"));
    expect(putConfig).toHaveBeenCalledWith(
      expect.objectContaining({
        backstage_token: "new-tok",
        server_token: "oadk_new",
      }),
    );
  });

  it("shows daemon errors and stays open", async () => {
    putConfig.mockRejectedValue(new Error("memory_repo must be owner/name"));
    const onDone = vi.fn();
    render(<Onboarding config={base} onDone={onDone} />);
    await userEvent.type(screen.getByTestId("ob-memory-repo"), "junk");
    await userEvent.click(screen.getByTestId("ob-save"));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "memory_repo must be owner/name",
    );
    expect(onDone).not.toHaveBeenCalled();
  });
});

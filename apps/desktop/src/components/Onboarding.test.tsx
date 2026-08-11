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
    await userEvent.click(screen.getByTestId("ob-save"));
    expect(putConfig).toHaveBeenCalledWith({
      backstage_base_url: "https://backstage.example.com",
      backstage_token: "tok",
      memory_repo: "acme/memory",
      onboarded: true,
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

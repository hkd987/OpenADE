import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SessionMeta } from "../api";
import { CommandPalette } from "./CommandPalette";

const sessions: SessionMeta[] = [
  {
    id: "s-1",
    title: "add retries",
    harness: "claude-code",
    repo_root: "/repos/checkout",
    state: "running",
    created_at: "",
    updated_at: "",
  },
  {
    id: "s-2",
    title: "migrate payments",
    harness: "codex-cli",
    repo_root: "/repos/payments",
    state: "completed",
    created_at: "",
    updated_at: "",
  },
];

function setup() {
  const onSelect = vi.fn();
  const onNewSession = vi.fn();
  const onClose = vi.fn();
  render(
    <CommandPalette
      sessions={sessions}
      onSelect={onSelect}
      onNewSession={onNewSession}
      onClose={onClose}
    />,
  );
  return { onSelect, onNewSession, onClose };
}

describe("CommandPalette", () => {
  it("lists new-session first, then every session", () => {
    setup();
    const items = screen.getAllByTestId("palette-item");
    expect(items[0]).toHaveTextContent("New session");
    expect(items[1]).toHaveTextContent("add retries");
    expect(items[2]).toHaveTextContent("migrate payments");
  });

  it("enter on an empty palette starts a new session", async () => {
    const { onNewSession, onClose } = setup();
    await userEvent.keyboard("{Enter}");
    expect(onNewSession).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it("typing filters and enter jumps to the first match", async () => {
    const { onSelect, onClose } = setup();
    await userEvent.type(screen.getByTestId("palette-input"), "payments");
    const items = screen.getAllByTestId("palette-item");
    expect(items[0]).toHaveTextContent("migrate payments");
    expect(items).toHaveLength(2); // match + trailing New session
    await userEvent.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledWith("s-2");
    expect(onClose).toHaveBeenCalled();
  });

  it("matches on project name too, and arrows move the selection", async () => {
    const { onSelect } = setup();
    await userEvent.type(screen.getByTestId("palette-input"), "checkout");
    expect(screen.getAllByTestId("palette-item")[0]).toHaveTextContent(
      "add retries",
    );
    // Arrow down to the New-session entry and back up to the match.
    await userEvent.keyboard("{ArrowDown}{ArrowUp}{Enter}");
    expect(onSelect).toHaveBeenCalledWith("s-1");
  });

  it("clicking an item activates it; clicking the overlay closes", async () => {
    const { onSelect, onClose } = setup();
    await userEvent.click(
      screen.getAllByTestId("palette-item").find((i) =>
        i.textContent?.includes("migrate payments"),
      )!,
    );
    expect(onSelect).toHaveBeenCalledWith("s-2");

    await userEvent.click(screen.getByTestId("palette"));
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("escape closes without selecting", async () => {
    const { onSelect, onClose } = setup();
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalled();
    expect(onSelect).not.toHaveBeenCalled();
  });
});

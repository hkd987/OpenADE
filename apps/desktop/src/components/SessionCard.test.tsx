import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SessionMeta } from "../api";
import { SessionCard } from "./SessionCard";

const session: SessionMeta = {
  id: "s-1",
  title: "add retries",
  harness: "claude-code",
  repo_root: "/repo",
  branch: "openade/add-retries-ab12",
  entity_ref: "component:default/payments-api",
  state: "needs-input",
  created_at: "2026-08-11T10:00:00Z",
  updated_at: "2026-08-11T10:05:00Z",
};

describe("SessionCard", () => {
  it("shows state, harness, entity chip, and branch", () => {
    render(<SessionCard session={session} selected={false} onSelect={() => {}} />);
    expect(screen.getByText("needs-input")).toBeInTheDocument();
    expect(screen.getByText("Claude Code")).toBeInTheDocument();
    // The memory chip splits the ref into a highlighted kind + the rest.
    const chip = screen.getByTestId("entity-chip");
    expect(chip).toHaveTextContent("component");
    expect(chip).toHaveTextContent("default/payments-api");
    expect(chip.className).toContain("entity-chip-component");
    expect(screen.getByText("openade/add-retries-ab12")).toBeInTheDocument();
  });

  it("marks repo entities as the GitHub memory source", () => {
    const repoSession: SessionMeta = {
      ...session,
      entity_ref: "repo:acme/payments-service",
    };
    render(
      <SessionCard session={repoSession} selected={false} onSelect={() => {}} />,
    );
    const chip = screen.getByTestId("entity-chip");
    expect(chip.className).toContain("entity-chip-repo");
    expect(chip).toHaveTextContent("repo");
    expect(chip).toHaveTextContent("acme/payments-service");
  });

  it("invokes onSelect and reflects selection", async () => {
    const onSelect = vi.fn();
    const { rerender } = render(
      <SessionCard session={session} selected={false} onSelect={onSelect} />,
    );
    await userEvent.click(screen.getByRole("button"));
    expect(onSelect).toHaveBeenCalledOnce();

    rerender(<SessionCard session={session} selected={true} onSelect={onSelect} />);
    expect(screen.getByRole("button").className).toContain("selected");
  });
});

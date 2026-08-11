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
  it("shows state, harness, entity, and branch", () => {
    render(<SessionCard session={session} selected={false} onSelect={() => {}} />);
    expect(screen.getByText("needs-input")).toBeInTheDocument();
    expect(screen.getByText("Claude Code")).toBeInTheDocument();
    expect(screen.getByText("component:default/payments-api")).toBeInTheDocument();
    expect(screen.getByText("openade/add-retries-ab12")).toBeInTheDocument();
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

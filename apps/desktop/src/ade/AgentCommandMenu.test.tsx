import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AgentCommandMenu, filterAgentCommands } from "./AgentCommandMenu";
import { AgentCommand } from "./api";

const commands: AgentCommand[] = [
  { id: "skill:review-pr", name: "review-pr", kind: "skill", source: "Codex", invocation: "$review-pr", description: "Review a pull request" },
  { id: "skill:fix-ci", name: "fix-ci", kind: "skill", source: "Project", invocation: "$fix-ci", description: "Fix failing CI" },
];

describe("AgentCommandMenu", () => {
  it("filters slash and skill syntax without provider punctuation", () => {
    expect(filterAgentCommands(commands, "$review")).toEqual([commands[0]]);
    expect(filterAgentCommands(commands, "/fix")).toEqual([commands[1]]);
  });

  it("inserts the selected provider invocation", () => {
    const onSelect = vi.fn();
    render(<AgentCommandMenu commands={commands} input="$review" onSelect={onSelect} />);
    fireEvent.click(screen.getByRole("option", { name: /review-pr/i }));
    expect(onSelect).toHaveBeenCalledWith(commands[0]);
  });
});

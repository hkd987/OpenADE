import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ChatTimeline } from "./ChatTimeline";
import { Session } from "./api";

const session = {
  id: "session-1",
  agent: "codex",
  status: "running",
  prompt: "Check this repository",
  created_at: new Date().toISOString(),
} as Session;

describe("ChatTimeline", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("keeps live work steps collapsed until the user opens them", () => {
    const output = [
      '{"type":"thread.started"}',
      '{"type":"item.started","item":{"type":"command_execution","command":"git status"}}',
    ].join("\n");
    const { container } = render(<ChatTimeline session={session} output={output} activityExpanded />);

    const group = container.querySelector("details.activity-group");
    expect(group).not.toHaveAttribute("open");
    fireEvent.click(group!.querySelector(":scope > summary")!);
    expect(group).toHaveAttribute("open");
  });

  it("progressively reveals a completed provider message while the run is active", async () => {
    vi.useFakeTimers();
    const output = '{"type":"item.completed","item":{"type":"agent_message","text":"Streaming into the conversation smoothly"}}';
    render(<ChatTimeline session={session} output={output} />);

    expect(screen.queryByText("Streaming into the conversation smoothly")).not.toBeInTheDocument();
    for (let index = 0; index < 30; index += 1) {
      await act(async () => vi.runOnlyPendingTimersAsync());
    }
    expect(screen.getByText("Streaming into the conversation smoothly")).toBeInTheDocument();
  });

  it("clears response and code-copy feedback timers when the chat closes", async () => {
    vi.useFakeTimers();
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const completed = { ...session, status: "completed" as const };
    const output = '{"type":"item.completed","item":{"type":"agent_message","text":"Done\\n\\n```ts\\nconst ready = true;\\n```"}}';
    const view = render(<ChatTimeline session={completed} output={output} />);

    fireEvent.click(screen.getByRole("button", { name: "Copy code" }));
    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    await act(async () => { await Promise.resolve(); });
    expect(writeText).toHaveBeenCalledTimes(2);
    expect(vi.getTimerCount()).toBeGreaterThan(0);

    view.unmount();
    expect(vi.getTimerCount()).toBe(0);
  });
});

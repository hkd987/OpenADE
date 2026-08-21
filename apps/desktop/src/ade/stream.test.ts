import { describe, expect, it } from "vitest";
import { formatAgentStream } from "./stream";

describe("formatAgentStream", () => {
  it("turns Codex JSONL into readable intent and response blocks", () => {
    const value = [
      '{"type":"thread.started","thread_id":"thread-1"}',
      '{"type":"turn.started"}',
      '{"type":"item.started","item":{"type":"command_execution","command":"git status"}}',
      '{"type":"item.completed","item":{"type":"command_execution","aggregated_output":"clean\\n"}}',
      '{"type":"item.completed","item":{"type":"agent_message","text":"Ready for review."}}',
    ].join("\n");
    expect(formatAgentStream(value)).toContain("Intent · git status");
    expect(formatAgentStream(value)).toContain("Ready for review.");
  });

  it("uses partial Claude text until the final response arrives", () => {
    const value = '{"type":"stream_event","event":{"delta":{"type":"text_delta","text":"Working"}}}';
    expect(formatAgentStream(value)).toBe("Working");
  });
});

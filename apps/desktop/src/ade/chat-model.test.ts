import { describe, expect, it } from "vitest";
import { parseChatTranscript } from "./chat-model";

describe("parseChatTranscript", () => {
  it("separates Codex activity from the Markdown response", () => {
    const transcript = [
      '{"type":"thread.started","thread_id":"thread-1"}',
      '{"type":"item.started","item":{"type":"command_execution","command":"git status"}}',
      '{"type":"item.completed","item":{"type":"command_execution","command":"git status","aggregated_output":"clean\\n"}}',
      '{"type":"item.completed","item":{"type":"agent_message","text":"## Ready\\n\\n- Tests pass\\n- Tree is clean"}}',
    ].join("\n");
    const turns = parseChatTranscript(transcript, "Check the repository", false);
    expect(turns).toHaveLength(2);
    expect(turns[1].markdown).toContain("## Ready");
    expect(turns[1].activities.map((item) => item.title)).toContain("Ran git status");
    expect(turns[1].markdown).not.toContain("git status");
  });

  it("shows Claude partial text while streaming and hides raw hook noise", () => {
    const transcript = [
      "Reading additional input from stdin...",
      '{"type":"stream_event","event":{"delta":{"type":"thinking_delta","thinking":"checking"}}}',
      '{"type":"stream_event","event":{"delta":{"type":"text_delta","text":"Working **now**"}}}',
      '{"type":"system","subtype":"hook_started","hook_name":"SessionStart"}',
    ].join("\n");
    const turns = parseChatTranscript(transcript, "Do the work", true);
    expect(turns.at(-1)).toMatchObject({ markdown: "Working **now**", streaming: true });
    expect(turns.at(-1)?.activities).toHaveLength(1);
  });

  it("creates another conversational turn for persisted follow-up markers", () => {
    const transcript = [
      '{"type":"result","result":"First answer"}',
      '{"type":"openade.user_message","text":"Follow up"}',
      '{"type":"result","result":"Second answer"}',
    ].join("\n");
    expect(parseChatTranscript(transcript, "Initial", false).map((turn) => turn.markdown)).toEqual([
      "Initial",
      "First answer",
      "Follow up",
      "Second answer",
    ]);
  });
});

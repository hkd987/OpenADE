export type ChatActivityKind = "thinking" | "command" | "tool" | "notice";

export interface ChatActivity {
  id: string;
  kind: ChatActivityKind;
  title: string;
  detail?: string;
}

export interface ChatTurn {
  id: string;
  role: "user" | "assistant";
  markdown: string;
  activities: ChatActivity[];
  streaming?: boolean;
}

type JsonRecord = Record<string, unknown>;

export function parseChatTranscript(
  value: string,
  initialPrompt: string,
  running: boolean,
): ChatTurn[] {
  const turns: ChatTurn[] = [];
  if (initialPrompt.trim()) {
    turns.push({ id: "user-0", role: "user", markdown: initialPrompt.trim(), activities: [] });
  }

  let assistant = newAssistant(0);
  let partial = "";
  let finalMessage = "";

  for (const rawLine of stripANSI(value).replace(/\r/g, "").split("\n")) {
    const line = rawLine.trim();
    if (!line.startsWith("{")) continue;
    const event = parseEvent(line);
    if (!event) continue;

    if (event.type === "openade.user_message") {
      commitAssistant(turns, assistant, finalMessage || partial);
      turns.push({
        id: `user-${turns.length}`,
        role: "user",
        markdown: String(event.text ?? "").trim(),
        activities: [],
      });
      assistant = newAssistant(turns.length);
      partial = "";
      finalMessage = "";
      continue;
    }

    const type = String(event.type ?? "");
    const item = isRecord(event.item) ? event.item : null;
    if (type === "thread.started" || type === "turn.started") {
      addActivity(assistant, "thinking", "Thinking");
    }
    if (type === "item.started" && item?.type === "command_execution") {
      addActivity(assistant, "command", commandTitle(String(item.command ?? "Run command")));
    }
    if (type === "item.completed" && item?.type === "command_execution") {
      const output = String(item.aggregated_output ?? "").trim();
      const command = String(item.command ?? "Command finished");
      completeActivity(assistant, "command", commandTitle(command), compactDetail(output));
    }
    if (type === "item.completed" && item?.type === "agent_message") {
      finalMessage = String(item.text ?? "").trim();
    }
    if (type === "item.completed" && item?.type === "error") {
      addActivity(assistant, "notice", "Agent notice", String(item.message ?? "Agent error"));
    }

    if (type === "stream_event" && isRecord(event.event)) {
      const delta = isRecord(event.event.delta) ? event.event.delta : null;
      if (delta?.type === "text_delta") partial += String(delta.text ?? "");
      if (delta?.type === "thinking_delta") addActivity(assistant, "thinking", "Thinking");
    }
    if (type === "assistant" && isRecord(event.message) && Array.isArray(event.message.content)) {
      const content = event.message.content.filter(isRecord);
      const text = content
        .filter((block) => block.type === "text")
        .map((block) => String(block.text ?? ""))
        .join("\n")
        .trim();
      if (text) finalMessage = text;
      for (const block of content.filter((entry) => entry.type === "tool_use")) {
        addActivity(assistant, "tool", toolTitle(String(block.name ?? "Use tool")));
      }
    }
    if (type === "result" && typeof event.result === "string" && event.result.trim()) {
      finalMessage = event.result.trim();
    }
  }

  assistant.streaming = running;
  commitAssistant(turns, assistant, finalMessage || partial, running);
  return turns;
}

function newAssistant(index: number): ChatTurn {
  return { id: `assistant-${index}`, role: "assistant", markdown: "", activities: [] };
}

function commitAssistant(
  turns: ChatTurn[],
  assistant: ChatTurn,
  markdown: string,
  force = false,
) {
  assistant.markdown = markdown.trim();
  if (force || assistant.markdown || assistant.activities.length) turns.push(assistant);
}

function addActivity(
  turn: ChatTurn,
  kind: ChatActivityKind,
  title: string,
  detail?: string,
) {
  const previous = turn.activities.at(-1);
  if (previous?.kind === kind && previous.title === title && previous.detail === detail) return;
  turn.activities.push({ id: `${turn.id}-activity-${turn.activities.length}`, kind, title, detail });
}

function completeActivity(
  turn: ChatTurn,
  kind: ChatActivityKind,
  title: string,
  detail?: string,
) {
  const existing = [...turn.activities].reverse().find((activity) => activity.kind === kind && activity.title === title);
  if (existing) {
    existing.detail = detail;
    return;
  }
  addActivity(turn, kind, title, detail);
}

function commandTitle(command: string): string {
  const singleLine = command.replace(/\s+/g, " ").trim();
  return singleLine.length > 74 ? `Ran ${singleLine.slice(0, 71)}…` : `Ran ${singleLine}`;
}

function toolTitle(name: string): string {
  const readable = name.replace(/[_-]+/g, " ").replace(/\b\w/g, (letter) => letter.toUpperCase());
  return readable || "Used a tool";
}

function compactDetail(value: string): string | undefined {
  if (!value) return undefined;
  const lines = value.split("\n").filter(Boolean);
  return lines.slice(-4).join("\n").slice(0, 600);
}

function parseEvent(line: string): JsonRecord | null {
  try {
    const value = JSON.parse(line) as unknown;
    return isRecord(value) ? value : null;
  } catch {
    return null;
  }
}

function isRecord(value: unknown): value is JsonRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function stripANSI(value: string): string {
  return value.replace(/\x1B(?:[@-_][0-?]*[ -/]*[@-~]|\][^\x07]*(?:\x07|\x1B\\))/g, "");
}

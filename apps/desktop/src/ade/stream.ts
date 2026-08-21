type JsonRecord = Record<string, unknown>;

export function formatAgentStream(value: string): string {
  const plain = stripANSI(value).replace(/\r/g, "");
  const events: string[] = [];
  let partial = "";
  let finalMessage = "";

  for (const rawLine of plain.split("\n")) {
    const line = rawLine.trim();
    if (!line) continue;
    const event = parseEvent(line);
    if (!event) {
      if (!line.startsWith("20") && !line.startsWith("Reading additional input")) events.push(line);
      continue;
    }

    const type = String(event.type ?? "");
    const item = isRecord(event.item) ? event.item : null;
    if (type === "thread.started") events.push(`Session · ${String(event.thread_id ?? "started")}`);
    if (type === "turn.started") events.push("Thinking…");
    if (type === "item.started" && item?.type === "command_execution") events.push(`Intent · ${String(item.command ?? "Run command")}`);
    if (type === "item.completed" && item?.type === "command_execution") {
      const output = String(item.aggregated_output ?? "").trim();
      if (output) events.push(output);
    }
    if (type === "item.completed" && item?.type === "agent_message") finalMessage = String(item.text ?? "").trim();
    if (type === "item.completed" && item?.type === "error") events.push(`Notice · ${String(item.message ?? "Agent error")}`);

    if (type === "stream_event" && isRecord(event.event)) {
      const delta = isRecord(event.event.delta) ? event.event.delta : null;
      if (delta?.type === "text_delta" || delta?.type === "thinking_delta") partial += String(delta.text ?? delta.thinking ?? "");
    }
    if (type === "assistant" && isRecord(event.message) && Array.isArray(event.message.content)) {
      const content = event.message.content as JsonRecord[];
      const text = content.filter((block) => block.type === "text").map((block) => String(block.text ?? "")).join("\n").trim();
      if (text) finalMessage = text;
      for (const block of content.filter((entry) => entry.type === "tool_use")) events.push(`Intent · ${String(block.name ?? "Use tool")}`);
    }
    if (type === "result" && typeof event.result === "string") finalMessage = event.result.trim();
  }

  const response = finalMessage || partial.trim();
  if (response) events.push(response);
  return dedupe(events).slice(-80).join("\n\n");
}

function parseEvent(line: string): JsonRecord | null {
  if (!line.startsWith("{")) return null;
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

function dedupe(values: string[]): string[] {
  return values.filter((value, index) => value && values[index - 1] !== value);
}

function stripANSI(value: string): string {
  return value.replace(/\x1B(?:[@-_][0-?]*[ -/]*[@-~]|\][^\x07]*(?:\x07|\x1B\\))/g, "");
}

import {
  CaretDown,
  Check,
  Copy,
  Cpu,
  Lightning,
  SpinnerGap,
  TerminalWindow,
  Wrench,
} from "@phosphor-icons/react";
import { useMemo, useState } from "react";
import { relativeTime, Session } from "./api";
import { ChatActivity, parseChatTranscript } from "./chat-model";
import { MarkdownMessage } from "./MarkdownMessage";

export function ChatTimeline({ session, output }: { session: Session; output: string }) {
  const running = ["starting", "running", "waiting"].includes(session.status);
  const turns = useMemo(
    () => parseChatTranscript(output, session.prompt, running),
    [output, running, session.prompt],
  );

  return (
    <div className="chat-timeline" aria-live="polite">
      {turns.map((turn, index) =>
        turn.role === "user" ? (
          <article className="chat-user-turn" key={turn.id}>
            <div>{turn.markdown}</div>
            <small>You · {index === 0 ? relativeTime(session.created_at) : "now"}</small>
          </article>
        ) : (
          <AssistantTurn
            key={turn.id}
            markdown={turn.markdown}
            activities={turn.activities}
            streaming={Boolean(turn.streaming)}
            agent={agentLabel(session.agent)}
          />
        ),
      )}
    </div>
  );
}

function AssistantTurn({
  markdown,
  activities,
  streaming,
  agent,
}: {
  markdown: string;
  activities: ChatActivity[];
  streaming: boolean;
  agent: string;
}) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    await navigator.clipboard.writeText(markdown);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1200);
  };
  return (
    <article className="chat-assistant-turn">
      <header><span className="agent-avatar"><Cpu weight="fill" /></span><strong>{agent}</strong></header>
      {activities.length > 0 && <ActivityGroup activities={activities} streaming={streaming} />}
      {markdown ? <MarkdownMessage>{markdown}</MarkdownMessage> : streaming ? (
        <div className="native-thinking"><SpinnerGap className="spin" /> Working through the task…</div>
      ) : null}
      {streaming && markdown && <span className="streaming-cursor" aria-label="Streaming" />}
      {markdown && !streaming && (
        <div className="response-actions">
          <button type="button" onClick={() => void copy()}>{copied ? <Check /> : <Copy />}<span>{copied ? "Copied" : "Copy"}</span></button>
        </div>
      )}
    </article>
  );
}

function ActivityGroup({ activities, streaming }: { activities: ChatActivity[]; streaming: boolean }) {
  return (
    <details className="activity-group" open={streaming}>
      <summary>
        {streaming ? <SpinnerGap className="spin" /> : <Check />}
        <span>{streaming ? activities.at(-1)?.title ?? "Working" : `${activities.length} work step${activities.length === 1 ? "" : "s"}`}</span>
        <CaretDown className="activity-caret" />
      </summary>
      <div className="activity-list">
        {activities.map((activity) => (
          <div className="activity-row" key={activity.id}>
            <span className={`activity-icon ${activity.kind}`}>{activityIcon(activity)}</span>
            <div><strong>{activity.title}</strong>{activity.detail && <pre>{activity.detail}</pre>}</div>
          </div>
        ))}
      </div>
    </details>
  );
}

function activityIcon(activity: ChatActivity) {
  if (activity.kind === "command") return <TerminalWindow />;
  if (activity.kind === "tool") return <Wrench />;
  return <Lightning />;
}

function agentLabel(agent: string): string {
  return ({ claude: "Claude Code", codex: "Codex CLI", copilot: "Copilot", opencode: "OpenCode" } as Record<string, string>)[agent] ?? agent;
}

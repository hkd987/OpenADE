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
import { useEffect, useMemo, useRef, useState } from "react";
import { relativeTime, Session } from "./api";
import { ChatActivity, parseChatTranscript } from "./chat-model";
import { MarkdownMessage } from "./MarkdownMessage";

export function ChatTimeline({ session, output, activityExpanded = false }: { session: Session; output: string; activityExpanded?: boolean }) {
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
            activityExpanded={activityExpanded}
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
  activityExpanded,
}: {
  markdown: string;
  activities: ChatActivity[];
  streaming: boolean;
  agent: string;
  activityExpanded: boolean;
}) {
  const [copied, setCopied] = useState(false);
  const copyTimerRef = useRef<number | undefined>(undefined);
  const mountedRef = useRef(false);
  const visibleMarkdown = useProgressiveMarkdown(markdown, streaming);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      if (copyTimerRef.current !== undefined) window.clearTimeout(copyTimerRef.current);
    };
  }, []);
  const copy = async () => {
    await navigator.clipboard.writeText(markdown);
    if (!mountedRef.current) return;
    setCopied(true);
    if (copyTimerRef.current !== undefined) window.clearTimeout(copyTimerRef.current);
    copyTimerRef.current = window.setTimeout(() => {
      copyTimerRef.current = undefined;
      setCopied(false);
    }, 1200);
  };
  return (
    <article className="chat-assistant-turn">
      <header><span className="agent-avatar"><Cpu weight="fill" /></span><strong>{agent}</strong></header>
      {activities.length > 0 && <ActivityGroup activities={activities} streaming={streaming} expanded={activityExpanded} />}
      {visibleMarkdown ? <MarkdownMessage>{visibleMarkdown}</MarkdownMessage> : streaming ? (
        <div className="native-thinking"><SpinnerGap className="spin" /> Working through the task…</div>
      ) : null}
      {streaming && visibleMarkdown && <span className="streaming-cursor" aria-label="Streaming" />}
      {markdown && !streaming && (
        <div className="response-actions">
          <button type="button" onClick={() => void copy()}>{copied ? <Check /> : <Copy />}<span>{copied ? "Copied" : "Copy"}</span></button>
        </div>
      )}
    </article>
  );
}

function ActivityGroup({ activities, streaming, expanded }: { activities: ChatActivity[]; streaming: boolean; expanded: boolean }) {
  const [open, setOpen] = useState(expanded && !streaming);
  useEffect(() => setOpen(streaming ? false : expanded), [expanded, streaming]);
  return (
    <details className="activity-group" open={open} onToggle={(event) => setOpen(event.currentTarget.open)}>
      <summary>
        {streaming ? <SpinnerGap className="spin" /> : <Check />}
        <span>{streaming ? activities.at(-1)?.title ?? "Working" : `${activities.length} work step${activities.length === 1 ? "" : "s"}`}</span>
        <CaretDown className="activity-caret" />
      </summary>
      <div className="activity-list">
        {activities.map((activity) => (
          <div className="activity-row" key={activity.id}>
            <span className={`activity-icon ${activity.kind}`}>{activityIcon(activity)}</span>
            {activity.detail ? <details className="activity-detail"><summary>{activity.title}</summary><pre>{activity.detail}</pre></details> : <strong>{activity.title}</strong>}
          </div>
        ))}
      </div>
    </details>
  );
}

function useProgressiveMarkdown(markdown: string, streaming: boolean): string {
  const [visible, setVisible] = useState(streaming ? "" : markdown);

  useEffect(() => {
    if (!streaming) {
      setVisible(markdown);
      return;
    }
    if (!markdown.startsWith(visible)) {
      setVisible("");
    }
  }, [markdown, streaming, visible]);

  useEffect(() => {
    if (!streaming || visible.length >= markdown.length) return;
    const remaining = markdown.length - visible.length;
    const step = Math.max(2, Math.min(28, Math.ceil(remaining / 18)));
    const timer = window.setTimeout(() => {
      setVisible(markdown.slice(0, Math.min(markdown.length, visible.length + step)));
    }, 18);
    return () => window.clearTimeout(timer);
  }, [markdown, streaming, visible]);

  return visible;
}

function activityIcon(activity: ChatActivity) {
  if (activity.kind === "command") return <TerminalWindow />;
  if (activity.kind === "tool") return <Wrench />;
  return <Lightning />;
}

function agentLabel(agent: string): string {
  return ({ claude: "Claude Code", codex: "Codex CLI", copilot: "Copilot", opencode: "OpenCode" } as Record<string, string>)[agent] ?? agent;
}

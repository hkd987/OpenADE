import {
  ArrowBendDownRight,
  ArrowBendUpLeft,
  DotsThree,
  Trash,
} from "@phosphor-icons/react";
import { QueuedMessage } from "./api";

export function MessageQueue({
  messages,
  sendingId,
  onSteer,
  onRemove,
  onEdit,
}: {
  messages: QueuedMessage[];
  sendingId: string | null;
  onSteer: (id: string) => void;
  onRemove: (id: string) => void;
  onEdit: (id: string) => void;
}) {
  if (messages.length === 0) return null;

  return (
    <section className="message-queue" aria-label="Queued messages">
      <span className="queue-progress">Step 1 / {messages.length + 1}</span>
      <div className="queue-list">
        {messages.map((message, index) => (
          <article className={`queue-item ${sendingId === message.id ? "sending" : ""}`} key={message.id}>
            <span className="queue-order" aria-hidden="true"><ArrowBendDownRight /></span>
            <span className="queue-index">{index + 1}</span>
            <p title={message.text}>{message.text}</p>
            <button type="button" className="queue-steer" onClick={() => onSteer(message.id)} disabled={sendingId === message.id} title="Move this message to the front of the queue">
              <ArrowBendUpLeft /> <span>{sendingId === message.id ? "Sending" : "Steer"}</span>
            </button>
            <button type="button" className="queue-action" onClick={() => onRemove(message.id)} disabled={sendingId === message.id} aria-label={`Remove queued message ${index + 1}`} title="Remove from queue"><Trash /></button>
            <button type="button" className="queue-action" onClick={() => onEdit(message.id)} disabled={sendingId === message.id} aria-label={`Edit queued message ${index + 1}`} title="Edit message"><DotsThree /></button>
          </article>
        ))}
      </div>
    </section>
  );
}

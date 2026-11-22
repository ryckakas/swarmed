import { type KeyboardEvent, useEffect, useState } from "react";
import { api } from "../api";
import type { QueuedMessage } from "../api/types";
import { useStore } from "../store";
import { useWords } from "../useWords";

/** Stands in for a queue that has not been read, without a new array on every render. */
const NONE: QueuedMessage[] = [];

interface Props {
    sessionId: number;
    /** Whether the agent is busy. While it is not, the queue does not move by itself. */
    busy: boolean;
}

/**
 * The messages written while the agent was busy, in the order they will be sent. Each can be
 * rewritten, taken out, or sent at once for as long as it waits; sent at once while the agent
 * is busy, it stops the turn under way.
 */
export function MessageQueue({ sessionId, busy }: Props) {
    const words = useWords();
    const queue = useStore((state) => state.queues[sessionId] ?? NONE);
    const setQueue = useStore((state) => state.setQueue);
    const report = useStore((state) => state.report);

    // Changes arrive as events; this is what was waiting before the session was opened.
    useEffect(() => {
        api.sessionQueue(sessionId)
            .then((messages) => setQueue(sessionId, messages))
            .catch(report);
    }, [sessionId, setQueue, report]);

    if (queue.length === 0) {
        return null;
    }
    const save = (message: QueuedMessage, text: string) => {
        api.sessionQueueEdit(sessionId, message.id, text)
            .then((messages) => setQueue(sessionId, messages))
            .catch(report);
    };
    const sendNow = (message: QueuedMessage) => {
        api.sessionSendNow(sessionId, message.id).catch(report);
    };
    const sendNowLabel = busy ? `Stop the ${words.terms.agent} and send this now` : "Send this now";
    return (
        <section className="queue" aria-label="Messages waiting to be sent">
            <div className="queue-head">
                <span>
                    {queue.length === 1 ? "1 message waiting" : `${queue.length} messages waiting`}
                    {busy ? `, sent when the ${words.terms.agent} is done` : ""}
                </span>
                {!busy && (
                    <button
                        type="button"
                        className="changes-refresh"
                        onClick={() => api.sessionSendQueued(sessionId).catch(report)}
                    >
                        Send the next one now
                    </button>
                )}
            </div>
            <ol className="queue-list">
                {queue.map((message) => (
                    <li key={message.id}>
                        <QueuedRow
                            message={message}
                            sendNowLabel={sendNowLabel}
                            onSave={(text) => save(message, text)}
                            onSendNow={() => sendNow(message)}
                        />
                    </li>
                ))}
            </ol>
        </section>
    );
}

interface RowProps {
    message: QueuedMessage;
    /** What sending the message at once does, which depends on whether the agent is busy. */
    sendNowLabel: string;
    onSave(text: string): void;
    onSendNow(): void;
}

function QueuedRow({ message, sendNowLabel, onSave, onSendNow }: RowProps) {
    // `null` while the message is only shown; the text being typed while it is edited.
    const [draft, setDraft] = useState<string | null>(null);
    if (draft === null) {
        return (
            <div className="queue-row">
                <button
                    type="button"
                    className="queue-text"
                    title="Edit this message"
                    onClick={() => setDraft(message.text)}
                >
                    {message.text}
                </button>
                <button
                    type="button"
                    className="queue-send"
                    aria-label={sendNowLabel}
                    title={sendNowLabel}
                    onClick={onSendNow}
                >
                    <svg viewBox="0 0 16 16" aria-hidden="true">
                        <path d="M8 12.5v-9M4 7.5l4-4 4 4" />
                    </svg>
                </button>
                <button
                    type="button"
                    className="queue-remove"
                    aria-label="Remove this message from the queue"
                    title="Remove from the queue"
                    onClick={() => onSave("")}
                >
                    ×
                </button>
            </div>
        );
    }
    const finish = (text: string | null) => {
        setDraft(null);
        if (text !== null && text !== message.text) {
            onSave(text);
        }
    };
    const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
        if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
            event.preventDefault();
            finish(draft);
        } else if (event.key === "Escape") {
            // Only the edit is given up: the agent is not stopped from in here.
            event.stopPropagation();
            finish(null);
        }
    };
    return (
        <textarea
            className="queue-edit"
            aria-label="Waiting message"
            rows={Math.min(8, draft.split("\n").length)}
            value={draft}
            // biome-ignore lint/a11y/noAutofocus: the box appears because the user chose to edit
            autoFocus
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={onKeyDown}
            onBlur={() => finish(draft)}
        />
    );
}

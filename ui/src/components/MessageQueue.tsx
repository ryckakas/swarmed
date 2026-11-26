import { type ClipboardEvent, type KeyboardEvent, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { QueuedMessage } from "../api/types";
import { editFor } from "../editing";
import { elementsIn } from "../elements";
import { githubOf, typedAgain } from "../githubMentions";
import { splitAppContexts, withAppContexts } from "../mcpApps";
import { mentionedIn } from "../mentions";
import { expandTokens, forgetTokens, nothingWritten, tokensAgain, withToken } from "../pastes";
import { editingAfter, queueHeading } from "../queue";
import { type Draft, filesWith, githubRead } from "../sending";
import { useStore } from "../store";
import { splitFiles } from "../transcript";
import { findSession, repositoryOf } from "../tree";
import { usePastedImages } from "../usePastedImages";
import { useWords } from "../useWords";
import { FileChips } from "./FileChips";

/** Stands in for a queue that has not been read, without a new array on every render. */
const NONE: QueuedMessage[] = [];

/**
 * How often an open edit box renews its hold on the message: well within the core's ten
 * seconds, after which a hold nobody renews lapses.
 */
const RENEW_MS = 3000;

interface Props {
    sessionId: number;
    /** Whether the agent is busy. While it is not, the queue does not move by itself. */
    busy: boolean;
    /** Whether the agent takes a message into the turn under way, which offers Tell now. */
    hearsMidTurn: boolean;
}

/**
 * The messages written while the agent was busy, in the order they will be sent. Each can be
 * rewritten, taken out, or sent at once for as long as it waits; sent at once while the agent
 * is busy, it stops the turn under way.
 */
export function MessageQueue({ sessionId, busy, hearsMidTurn }: Props) {
    const words = useWords();
    const queue = useStore((state) => state.queues[sessionId] ?? NONE);
    // The message open for editing, which the queue does not send until the edit is over.
    const [editing, setEditing] = useState<number | null>(null);
    const queueAnswer = useStore((state) => state.queueAnswer);
    const report = useStore((state) => state.report);

    // Changes arrive as events; this is what was waiting before the session was opened.
    useEffect(() => {
        queueAnswer(sessionId, api.sessionQueue(sessionId)).catch(report);
    }, [sessionId, queueAnswer, report]);

    if (queue.length === 0) {
        return null;
    }
    const save = (message: QueuedMessage, edited: Edited) => {
        const { text, files, sessions, github, elements } = edited;
        const answer = api.sessionQueueEdit(
            sessionId,
            message.id,
            text,
            files,
            sessions,
            github,
            elements,
        );
        queueAnswer(sessionId, answer).catch(report);
    };
    const sendNow = (message: QueuedMessage) => {
        api.sessionSendNow(sessionId, message.id).catch(report);
    };
    const sendNowLabel = busy ? `Stop the ${words.terms.agent} and send this now` : "Send this now";
    const tellNow = (message: QueuedMessage) => {
        api.sessionTellNow(sessionId, message.id).catch(report);
    };
    // Only into a turn under way, and only an agent that takes one there.
    const tells = busy && hearsMidTurn;
    return (
        <section className="queue" aria-label="Messages waiting to be sent">
            <div className="queue-head">
                <span>{queueHeading(queue, busy, editing, words)}</span>
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
                            sessionId={sessionId}
                            message={message}
                            sendNowLabel={sendNowLabel}
                            onSave={(edited) => save(message, edited)}
                            onSendNow={() => sendNow(message)}
                            onTellNow={tells ? () => tellNow(message) : undefined}
                            onEditing={(open) =>
                                setEditing((was) => editingAfter(was, message.id, open))
                            }
                        />
                    </li>
                ))}
            </ol>
        </section>
    );
}

/**
 * A waiting message as it is to be sent now: its words, its files, and the sessions, pull
 * requests, issues and picked elements it mentions.
 */
interface Edited {
    text: string;
    files: string[];
    sessions: number[];
    github: number[];
    elements: number[];
}

/** No words and nothing attached: what takes a message out of the queue. */
const TAKEN_OUT: Edited = { text: "", files: [], sessions: [], github: [], elements: [] };

interface RowProps {
    sessionId: number;
    message: QueuedMessage;
    /** What sending the message at once does, which depends on whether the agent is busy. */
    sendNowLabel: string;
    onSave(edited: Edited): void;
    onSendNow(): void;
    /** Tells the message to the agent without stopping its turn; absent where it cannot. */
    onTellNow: (() => void) | undefined;
    /** The row opened or closed its edit box. */
    onEditing(open: boolean): void;
}

/**
 * Holds the message in the core for as long as `open`, renewing the hold, so that the queue
 * does not send it in the middle of an edit. Closing lets go: a send held back for the edit
 * goes then, with whatever text the edit saved before.
 */
function useHold(sessionId: number, message: number, open: boolean) {
    const report = useStore((state) => state.report);
    useEffect(() => {
        if (!open) {
            return;
        }
        const hold = () => {
            api.sessionQueueHold(sessionId, message).catch(report);
        };
        hold();
        const renewing = setInterval(hold, RENEW_MS);
        return () => {
            clearInterval(renewing);
            api.sessionQueueLetGo(sessionId, message).catch(report);
        };
    }, [sessionId, message, open, report]);
}

function QueuedRow(props: RowProps) {
    const { sessionId, message, sendNowLabel, onSave, onSendNow, onTellNow, onEditing } = props;
    // `null` while the message is only shown; what is being written while it is edited, its
    // files and images apart from its text, as in the message box.
    const [draft, setDraft] = useState<Draft | null>(null);
    const open = draft !== null;
    useHold(sessionId, message.id, open);
    // What it was opened with, to tell an edit that changed nothing.
    const opened = useRef<Draft | null>(null);
    const split = splitFiles(message.text);
    // The sessions, pull requests and issues it mentions stand as their tokens again, as typed.
    const said = { ...split, text: typedAgain(split.text) };
    // What apps left for the agent with it is not the user's to edit, and goes with it still.
    const { contexts } = splitAppContexts(said.text);
    if (draft === null) {
        // Shown as it was typed: its pastes and quotes as tokens, not in full.
        const shown = tokensAgain(said.text, nothingWritten());
        return (
            <div className="queue-row">
                <div className="queue-said">
                    <button
                        type="button"
                        className="queue-text"
                        title="Edit this message"
                        onClick={() => {
                            // Its pastes and quotes behind tokens again, sent in full once saved.
                            const editing = { ...said, text: tokensAgain(said.text), images: [] };
                            opened.current = editing;
                            setDraft(editing);
                            onEditing(true);
                        }}
                    >
                        {shown || "Files only"}
                    </button>
                    <FileChips files={said.files} sessionId={sessionId} />
                </div>
                {onTellNow && <TellNow onClick={onTellNow} />}
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
                    onClick={() => onSave(TAKEN_OUT)}
                >
                    ×
                </button>
            </div>
        );
    }
    const close = () => {
        setDraft(null);
        onEditing(false);
    };
    return (
        <QueueEdit
            sessionId={sessionId}
            draft={draft}
            onChange={setDraft}
            onDone={(edited) => {
                close();
                if (!(opened.current && unchanged(opened.current, edited))) {
                    onSave({ ...edited, text: withAppContexts(edited.text, contexts) });
                }
            }}
            onCancel={close}
        />
    );
}

/** Whether an edit left the message as it was opened: the same words, and the same files. */
function unchanged(opened: Draft, edited: Edited): boolean {
    const files = opened.files;
    const sameFiles =
        files.length === edited.files.length &&
        files.every((file, at) => edited.files[at] === file);
    return expandTokens(opened.text) === edited.text && sameFiles;
}

interface EditProps {
    sessionId: number;
    draft: Draft;
    onChange(draft: Draft): void;
    /** The edit is over, with the message as it is to be sent now. */
    onDone(edited: Edited): void;
    onCancel(): void;
}

/**
 * The box a waiting message is edited in: its files and images above its text, each with a
 * way to take it off, and images can be pasted in. Images pasted are kept as the session's
 * files before the edit is over, so that the message never waits without them.
 */
function QueueEdit({ sessionId, draft, onChange, onDone, onCancel }: EditProps) {
    const report = useStore((state) => state.report);
    // The repository whose pull requests and issues the message may name.
    const project = useStore((state) => {
        const session = findSession(state.projects, sessionId);
        return session && repositoryOf(state.projects, session);
    });
    // Keeping images takes a moment: Enter and leaving the box then must not save twice.
    const finishing = useRef(false);
    const takeImages = usePastedImages(draft.images, (images) => onChange({ ...draft, images }));
    const finish = () => {
        if (finishing.current) {
            return;
        }
        finishing.current = true;
        const github = githubOf(draft.text, project);
        // What it names on GitHub is read first, as when it was queued.
        Promise.all([filesWith(api, sessionId, draft), githubRead(api, project, github)])
            .then(([files]) => {
                // The core lists the files, the sessions, the pull requests and the issues under
                // the words, as it did before.
                const text = expandTokens(draft.text.trim());
                const elements = elementsIn(draft.text);
                onDone({ text, files, sessions: mentionedIn(draft.text), github, elements });
                forgetTokens(draft.text);
            })
            .catch((error: unknown) => {
                // Left open, to try again or give up.
                finishing.current = false;
                report(error);
            });
    };
    const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
        if (removedToken(event, draft.text, (text) => onChange({ ...draft, text }))) {
            return;
        }
        if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
            event.preventDefault();
            finish();
        } else if (event.key === "Escape") {
            // Only the edit is given up: the agent is not stopped from in here.
            event.stopPropagation();
            finishing.current = true;
            forgetTokens(draft.text);
            onCancel();
        }
    };
    const onPaste = (event: ClipboardEvent<HTMLTextAreaElement>) => {
        const images = takeImages(event.clipboardData);
        if (images && event.clipboardData.getData("text/plain") === "") {
            event.preventDefault();
        }
    };
    return (
        <div className="queue-editing">
            <FileChips
                files={draft.files}
                onRemove={(file) =>
                    onChange({ ...draft, files: draft.files.filter((other) => other !== file) })
                }
                sessionId={sessionId}
                images={draft.images}
                onRemoveImage={(image) =>
                    onChange({ ...draft, images: draft.images.filter((other) => other !== image) })
                }
            />
            <textarea
                className="queue-edit"
                aria-label="Waiting message"
                rows={Math.min(8, draft.text.split("\n").length)}
                value={draft.text}
                // biome-ignore lint/a11y/noAutofocus: the box appears because the user chose to edit
                autoFocus
                onChange={(event) => onChange({ ...draft, text: event.target.value })}
                onKeyDown={onKeyDown}
                onPaste={onPaste}
                onBlur={finish}
            />
        </div>
    );
}

/**
 * Takes a paste's or a quote's token out whole, for Backspace right after it or Delete right
 * before it, as the message box does. Says whether the key did that.
 */
function removedToken(
    event: KeyboardEvent<HTMLTextAreaElement>,
    text: string,
    onText: (text: string) => void,
): boolean {
    const box = event.currentTarget;
    const edit = editFor(event, text, box.selectionStart, box.selectionEnd);
    if (edit?.kind !== "remove") {
        return false;
    }
    event.preventDefault();
    const changed = withToken(text, edit.start, edit.end, "");
    onText(changed.value);
    requestAnimationFrame(() => box.setSelectionRange(changed.caret, changed.caret));
    return true;
}

/**
 * Hands the message to the agent in the middle of its turn, which it takes in at its next
 * step and goes on: a hint, without throwing away what the turn was doing.
 */
function TellNow({ onClick }: { onClick(): void }) {
    const words = useWords();
    const label = `Tell the ${words.terms.agent} now, without stopping it: it takes this in at its next step`;
    return (
        <button
            type="button"
            className="queue-tell"
            aria-label={label}
            title={label}
            onClick={onClick}
        >
            Tell now
        </button>
    );
}

import { useLayoutEffect, useMemo, useRef } from "react";
import { forkSession } from "../actions";
import type { AgentEvent } from "../api/types";
import { messageParts } from "../pastes";
import { useStore } from "../store";
import { activity, type Item, PLAN_TOOL, summarize, toItems } from "../transcript";
import { formatDuration } from "../turns";
import { useRoll } from "../useRoll";
import { useWords } from "../useWords";
import type { Words } from "../wording";
import { activityWords, pickWord, wordsFor } from "../wording";
import { ConfigProposal } from "./ConfigProposal";
import { FileChips } from "./FileChips";
import { HtmlView } from "./HtmlView";
import { Markdown } from "./Markdown";
import { PermissionPrompt } from "./PermissionPrompt";
import { Spawning } from "./Spawning";
import { ToolCall } from "./ToolCall";

/** How close to the bottom, in pixels, still counts as "following the conversation". */
const FOLLOW_DISTANCE = 80;

interface Props {
    sessionId: number;
    events: AgentEvent[];
    /** Whether the agent is busy on a turn; the transcript then ends with what it is doing. */
    working: boolean;
}

export function Transcript({ sessionId, events, working }: Props) {
    const items = useMemo(() => toItems(events), [events]);
    const scroller = useRef<HTMLDivElement>(null);
    // Each session gets its own Transcript (keyed by the caller), so this starts out true.
    const following = useRef(true);
    const knownHeight = useRef(0);

    // Keep the latest message in view, unless the user scrolled up to read something. Rows
    // out of view have only an estimated height, so the list also grows as they are measured:
    // watching its size keeps the view pinned through that too.
    useLayoutEffect(() => {
        const element = scroller.current;
        const list = element?.firstElementChild;
        if (!(element && list)) {
            return;
        }
        const pin = () => {
            if (following.current) {
                element.scrollTop = element.scrollHeight;
            }
            knownHeight.current = element.scrollHeight;
        };
        pin();
        const observer = new ResizeObserver(pin);
        observer.observe(list);
        return () => observer.disconnect();
    }, []);

    const onScroll = () => {
        const element = scroller.current;
        // A scroll event with a new content height is the browser adjusting to rows being
        // measured, not the user scrolling; only the user can stop the view from following.
        if (element && element.scrollHeight === knownHeight.current) {
            const distance = element.scrollHeight - element.scrollTop - element.clientHeight;
            following.current = distance < FOLLOW_DISTANCE;
        }
    };

    return (
        <div className="transcript" ref={scroller} onScroll={onScroll}>
            <ol className="items">
                {items.map((item) => (
                    <li key={item.key} className={`item item-${item.kind}`}>
                        <ItemView item={item} sessionId={sessionId} />
                    </li>
                ))}
                {working && (
                    <li className="activity" aria-live="polite">
                        <Spawning />
                        <Activity events={events} />
                    </li>
                )}
            </ol>
        </div>
    );
}

/**
 * What the user wrote, with anything they pasted large folded away under a line that says how
 * much there is of it.
 */
function Said({ text }: { text: string }) {
    return messageParts(text).map((part, index) =>
        part.kind === "text" ? (
            // biome-ignore lint/suspicious/noArrayIndexKey: the parts of a sent message never change
            <span key={index}>{part.text}</span>
        ) : (
            // biome-ignore lint/suspicious/noArrayIndexKey: the parts of a sent message never change
            <details key={index} className="pasted">
                <summary>
                    Pasted text, {part.lines} {part.lines === 1 ? "line" : "lines"}
                </summary>
                <pre>{part.text}</pre>
            </details>
        ),
    );
}

function ItemView({ item, sessionId }: { item: Item; sessionId: number }) {
    switch (item.kind) {
        case "user":
            return (
                <div className="said">
                    <Said text={item.text} />
                    <FileChips files={item.files} />
                    <button
                        type="button"
                        className="said-fork"
                        title="Starts a new session with the conversation as it stood before this message"
                        onClick={() => void forkSession(sessionId, item.key)}
                    >
                        Fork from here
                    </button>
                </div>
            );
        case "text":
            return <Markdown text={item.text} streaming={item.streaming} />;
        case "tool":
            return <ToolCall name={item.name} input={item.input} result={item.result} />;
        case "permission":
            return (
                <PermissionPrompt
                    sessionId={sessionId}
                    requestId={item.requestId}
                    toolName={item.toolName}
                    input={item.input}
                    description={item.description}
                />
            );
        case "proposal":
            return (
                <ConfigProposal
                    requestId={item.requestId}
                    title={item.title}
                    detail={item.detail}
                    accepted={item.accepted}
                />
            );
        case "denied":
            if (item.toolName === PLAN_TOOL) {
                return <p className="asked">You sent the plan back for changes.</p>;
            }
            return (
                <p className="asked">
                    You denied {item.toolName}:{" "}
                    <span className="mono">{summarize(item.input)}</span>
                </p>
            );
        case "peer":
            return (
                <div className="said said-peer">
                    <p className="said-from">
                        From session {item.from}
                        {item.title && `, ${item.title}`}
                    </p>
                    <Markdown text={item.text} />
                </div>
            );
        case "view":
            return <HtmlView html={item.html} />;
        case "terminal":
            return (
                <details className="shared-terminal">
                    <summary>Terminal output sent to the agent with this message</summary>
                    <pre className="tool-body">{item.text}</pre>
                </details>
            );
        case "session":
            return (
                <p className="aside">
                    {item.model}, {item.login} login, {item.mcp}, {item.skills} skills
                </p>
            );
        case "turn":
            return (
                <p className="aside turn-end">
                    <TurnEnd isError={item.isError} durationMs={item.durationMs} />
                    {item.costUsd !== null && `, $${item.costUsd.toFixed(2)} so far`}
                </p>
            );
        case "forked":
            return (
                <p className="forked-line">
                    Forked from session {item.from}. What stands above was said there; this
                    session's agent is given it with your first message.
                </p>
            );
        case "notice":
            return (
                <p className={`notice-line notice-${item.tone}`} role="alert">
                    {item.text}
                </p>
            );
    }
}

/** What the agent is doing right now, in the words of the style in use. */
function Activity({ events }: { events: AgentEvent[] }) {
    const plain = activity(events);
    const words = useStore((state) => wordsFor(state.settings?.style));
    const options = activityWords(words, plain);
    return <>{pickWord(options, useRoll(options.length > 1))}…</>;
}

/** How a turn's end reads: whether it finished, and how long it took if that was measured. */
export function turnEnd(words: Words, isError: boolean, durationMs: number | null): string {
    const how = isError ? words.turnStopped : words.turnDone;
    if (durationMs === null) {
        return how;
    }
    return `${how} ${isError ? "after" : "in"} ${formatDuration(durationMs)}`;
}

function TurnEnd({ isError, durationMs }: { isError: boolean; durationMs: number | null }) {
    return turnEnd(useWords(), isError, durationMs);
}

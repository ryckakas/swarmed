import { memo, useLayoutEffect, useMemo, useRef, useState } from "react";
import { forkFromMessage } from "../actions";
import { api } from "../api";
import type { AgentEvent } from "../api/types";
import { messageParts } from "../pastes";
import { screenHtml } from "../screens";
import { useStore } from "../store";
import {
    activity,
    type Item,
    latestMove,
    movedSaid,
    PLAN_TOOL,
    shownLines,
    shownTools,
    summarize,
    toItems,
} from "../transcript";
import { findSession, findSessionHome } from "../tree";
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
import { QuestionPrompt } from "./QuestionPrompt";
import { Spawning, Swarming } from "./Spawning";
import { ToolCall, ToolRun } from "./ToolCall";

/** How close to the bottom, in pixels, still counts as "following the conversation". */
const FOLLOW_DISTANCE = 80;

interface Props {
    sessionId: number;
    events: AgentEvent[];
    /** Whether the agent is busy on a turn; the transcript then ends with what it is doing. */
    working: boolean;
    /**
     * Whether the session's agent can send the work a call waits on to the background, which
     * its calls then offer while it works.
     */
    sendsToBackground: boolean;
}

export function Transcript({ sessionId, events, working, sendsToBackground }: Props) {
    const words = useWords();
    const toolCalls = useStore((state) => state.settings?.tool_calls ?? "folded");
    const turnEnds = useStore((state) => state.settings?.turn_end_lines ?? true);
    const items = useMemo(
        () => shownTools(shownLines(toItems(events, words), turnEnds), toolCalls),
        [events, words, turnEnds, toolCalls],
    );
    const lastMove = useMemo(() => latestMove(items), [items]);
    const sendsFrom = working && sendsToBackground ? sessionId : null;
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
                        <ItemView
                            item={item}
                            sessionId={sessionId}
                            latest={item.key === lastMove}
                            sendsFrom={sendsFrom}
                        />
                    </li>
                ))}
                {working && (
                    <li className="activity" aria-live="polite">
                        <Spawning />
                        <Activity events={events} />
                    </li>
                )}
                {working && toolCalls === "hidden" && (
                    <li className="activity-large">
                        <Swarming />
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

interface ItemProps {
    item: Item;
    sessionId: number;
    /** Whether this is the conversation's latest move of the worktree. */
    latest: boolean;
    /** The session whose calls' work can be sent to the background now; `null` for none. */
    sendsFrom: number | null;
}

function ItemView({ item, sessionId, latest, sendsFrom }: ItemProps) {
    const words = useWords();
    switch (item.kind) {
        case "user":
            return (
                <div className="said">
                    {item.remote && <p className="said-from">Typed on claude.ai</p>}
                    <Said text={item.text} />
                    <FileChips files={item.files} />
                    <button
                        type="button"
                        className="said-fork"
                        title={words.forkHereHint}
                        onClick={() => void forkFromMessage(sessionId, item.key)}
                    >
                        {words.forkHere}
                    </button>
                </div>
            );
        case "text":
            return <Markdown text={item.text} streaming={item.streaming} />;
        case "tool":
            return (
                <ToolCall
                    id={item.id}
                    name={item.name}
                    input={item.input}
                    result={item.result}
                    moved={item.moved}
                    sendsFrom={sendsFrom}
                />
            );
        case "tools":
            return <ToolRun calls={item.calls} sendsFrom={sendsFrom} />;
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
                        {words.fromSession(item.from)}
                        {item.title && `, ${item.title}`}
                    </p>
                    <Markdown text={item.text} />
                </div>
            );
        case "view":
            return <HtmlView html={item.html} />;
        case "screen":
            return item.command === "usage" ? (
                <>
                    <ScreenView item={item} />
                    <UsageCredits sessionId={sessionId} />
                </>
            ) : (
                <ScreenView item={item} />
            );
        case "asked":
            if (item.said !== null) {
                return (
                    <p className="asked">
                        <span className="mono">/{item.command}</span> {item.said}
                    </p>
                );
            }
            return (
                <QuestionPrompt
                    sessionId={sessionId}
                    requestId={item.requestId}
                    questions={item.questions}
                    command={item.command}
                />
            );
        case "terminal-only":
            return <TerminalOnly sessionId={sessionId} command={item.command} />;
        case "moved":
            return <Moved sessionId={sessionId} item={item} latest={latest} />;
        case "remote":
            return <RemoteLine item={item} />;
        case "terminal":
            return (
                <details className="shared-terminal">
                    <summary>
                        Terminal output sent to the {words.terms.agent} with this message
                    </summary>
                    <pre className="tool-body">{item.text}</pre>
                </details>
            );
        case "session":
            return (
                <p className="aside">
                    {item.model}, {item.login} login, {item.mcp}, {item.skills} {words.terms.skills}
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
            return <p className="forked-line">{words.forkedFrom(item.from)}</p>;
        case "notice":
            return (
                <p className={`notice-line notice-${item.tone}`} role="alert">
                    {item.text}
                </p>
            );
    }
}

/**
 * Remote Control turned on, with where the session is on claude.ai, or off, or failed: never
 * turned on, or ended after it was, which Claude Code reports alike.
 */
function RemoteLine({ item }: { item: Extract<Item, { kind: "remote" }> }) {
    const { url } = item;
    if (item.state === "failed") {
        const because = item.detail ? `: ${item.detail}` : ".";
        return (
            <p className="notice-line notice-warning" role="alert">
                Remote Control is not on{because}
            </p>
        );
    }
    if (item.state === "off" || url === null) {
        return <p className="aside">Remote Control is off.</p>;
    }
    return (
        <p className="aside remote-line">
            Remote Control is on: carry on from the Claude app, or at{" "}
            <button type="button" className="link-button" onClick={() => void api.openLink(url)}>
                {url}
            </button>
        </p>
    );
}

/**
 * A command whose screen Claude Code has only in its own terminal, with the button that opens
 * Claude Code there, in the session's terminal and apart from its shell.
 */
function TerminalOnly({ sessionId, command }: { sessionId: number; command: string }) {
    const words = useWords();
    const openClaudeTerminal = useStore((state) => state.openClaudeTerminal);
    return (
        <div className="terminal-only">
            <p className="asked">
                <span className="mono">/{command}</span> only works in Claude Code's own terminal,
                where it opens a screen.
            </p>
            <button
                type="button"
                className="button"
                onClick={() => openClaudeTerminal(sessionId, command)}
            >
                {words.openInClaude}
            </button>
        </div>
    );
}

/**
 * Says that the task's worktree moved to another branch. When the agent moved it, the way back
 * is offered: the same switch as the Publish panel's Branch menu. Only by the latest move: an
 * earlier one's way back would switch away from wherever the worktree has been put since.
 */
function Moved(props: {
    sessionId: number;
    item: Extract<Item, { kind: "moved" }>;
    latest: boolean;
}) {
    const { sessionId, item, latest } = props;
    const words = useWords();
    const projects = useStore((state) => state.projects);
    const report = useStore((state) => state.report);
    const worktreeMoved = useStore((state) => state.worktreeMoved);
    const [state, setState] = useState<"ready" | "switching" | "done">("ready");
    const session = findSession(projects, sessionId);
    const home = session && findSessionHome(projects, session);
    const back = item.from;
    const offered = latest && !item.betweenTurns && back !== null && home !== undefined;
    const switchBack = () => {
        if (back === null || home === undefined) {
            return;
        }
        setState("switching");
        api.taskSwitch(home.project.name, home.task.name, back)
            .then(() => {
                worktreeMoved(home.task.id);
                setState("done");
            })
            .catch((error: unknown) => {
                report(error);
                setState("ready");
            });
    };
    return (
        <div className="worktree-moved">
            <p className="asked">{movedSaid(item.from, item.to, item.betweenTurns, words)}</p>
            {offered && state !== "done" && (
                <button
                    type="button"
                    className="button"
                    disabled={state === "switching"}
                    title={`Runs git switch ${back}. Changes that are not ${words.terms.committed} come along unless they conflict.`}
                    onClick={switchBack}
                >
                    Switch back to {back}
                </button>
            )}
            {state === "done" && <p className="asked">Back on {back}.</p>}
        </div>
    );
}

/**
 * Under the usage screen, the way to what it leaves to Claude Code's own: buying usage credits
 * and setting their monthly cap, with `/usage-credits` opened in the session's terminal.
 */
function UsageCredits({ sessionId }: { sessionId: number }) {
    const words = useWords();
    const openClaudeTerminal = useStore((state) => state.openClaudeTerminal);
    return (
        <div className="terminal-only">
            <p className="asked">
                <span className="mono">/usage-credits</span> buys usage credits and sets their cap
                in Claude Code's own terminal.
            </p>
            <button
                type="button"
                className="button"
                onClick={() => openClaudeTerminal(sessionId, "usage-credits")}
            >
                {words.openInClaude}
            </button>
        </div>
    );
}

/**
 * One of the agent's own screens, drawn from what the agent said is on it. The transcript's
 * items are built again for every piece of a reply; the screen's parts are the event's own and
 * stay the same object, so its HTML, and with it the frame, is only built once.
 */
const ScreenView = memo(
    function ScreenView({ item }: { item: Extract<Item, { kind: "screen" }> }) {
        const html = useMemo(
            () => screenHtml({ note: item.note, sections: item.sections }),
            [item.note, item.sections],
        );
        return <HtmlView html={html} label={`/${item.command}: ${item.title}`} />;
    },
    (before, after) =>
        before.item.sections === after.item.sections && before.item.note === after.item.note,
);

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

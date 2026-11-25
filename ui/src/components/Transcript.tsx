import { memo, useMemo, useRef, useState } from "react";
import { editFromMessage, forkFromMessage } from "../actions";
import { api } from "../api";
import type { AgentEvent } from "../api/types";
import { elementsApart, type ListedElement } from "../elements";
import { type GitHubMentioned, githubApart } from "../githubMentions";
import { type Mentioned, mentionedApart } from "../mentions";
import { type MessagePart, messageParts } from "../pastes";
import { failedChecks, pullChangeSaid } from "../pulls";
import { offersPutAway } from "../putAway";
import { resetWhen } from "../rateLimit";
import { commitsSaid, messageWords, rewoundSaid } from "../rewind";
import { screenHtml } from "../screens";
import { useStore } from "../store";
import {
    activity,
    continueOffer,
    type Item,
    type LimitItem,
    latestFailure,
    latestFilesRewind,
    latestLimit,
    latestMerge,
    latestMove,
    movedSaid,
    PLAN_TOOL,
    type RewoundItem,
    shownLines,
    shownTools,
    summarize,
    toItems,
} from "../transcript";
import { findSession, findSessionHome } from "../tree";
import { formatDuration } from "../turns";
import { useRedrawAt } from "../useRedrawAt";
import { useRoll } from "../useRoll";
import { useScrollSpot } from "../useScrollSpot";
import { useWords } from "../useWords";
import type { Words } from "../wording";
import { activityWords, pickWord, wordsFor } from "../wording";
import { ConfigProposal } from "./ConfigProposal";
import { FileChips } from "./FileChips";
import { HtmlView } from "./HtmlView";
import { Markdown } from "./Markdown";
import { MentionList, WithMentions } from "./MentionChips";
import { PermissionPrompt } from "./PermissionPrompt";
import { SendFailure } from "./PullRequest";
import { PutAwayButton } from "./PutAway";
import { QuestionPrompt } from "./QuestionPrompt";
import { QuoteButton } from "./QuoteButton";
import { Spawning, Swarming } from "./Spawning";
import { ToolCall, ToolRun } from "./ToolCall";
import { LastImage } from "./ToolImages";

interface Props {
    sessionId: number;
    /** What its scroll position is kept under (`scrollKey`): the session's, or a sub-agent's. */
    place: string;
    events: AgentEvent[];
    /** Whether the agent is busy on a turn; the transcript then ends with what it is doing. */
    working: boolean;
    /**
     * Whether the session's agent can send the work a call waits on to the background, which
     * its calls then offer while it works.
     */
    sendsToBackground: boolean;
    /**
     * Whether words of the agent's replies can be quoted into the session's message box: in
     * the session's own transcript, not in a sub-agent's.
     */
    quotable?: boolean;
}

export function Transcript(props: Props) {
    const { sessionId, place, events, working, sendsToBackground, quotable = false } = props;
    const words = useWords();
    const toolCalls = useStore((state) => state.settings?.tool_calls ?? "folded");
    const turnEnds = useStore((state) => state.settings?.turn_end_lines ?? true);
    const items = useMemo(
        () => shownTools(shownLines(toItems(events, words), turnEnds), toolCalls),
        [events, words, turnEnds, toolCalls],
    );
    const lastMove = useMemo(() => latestMove(items), [items]);
    const lastFailure = useMemo(() => latestFailure(items), [items]);
    const lastLimit = useMemo(() => latestLimit(items), [items]);
    const lastMerge = useMemo(() => latestMerge(items), [items]);
    const lastFilesRewind = useMemo(() => latestFilesRewind(items), [items]);
    const sendsFrom = working && sendsToBackground ? sessionId : null;
    const scroller = useRef<HTMLDivElement>(null);
    const frame = useRef<HTMLDivElement>(null);
    // The latest message stays in view, unless the user scrolled up to read something; and a
    // transcript opened again comes back to where it was left.
    const { atEnd, toLatest, onScroll } = useScrollSpot(scroller, sessionId, place);

    return (
        <div className="transcript-frame" ref={frame}>
            <div className="transcript" ref={scroller} onScroll={onScroll}>
                <ol className="items">
                    {items.map((item) => (
                        <li
                            key={item.key}
                            data-key={item.key}
                            className={`item item-${item.kind}`}
                            // Only the agent's own replies are quoted from (`QUOTE_INTO`).
                            data-quote-into={
                                quotable && item.kind === "text" ? sessionId : undefined
                            }
                        >
                            <ItemView
                                item={item}
                                sessionId={sessionId}
                                latest={[
                                    lastMove,
                                    lastLimit,
                                    lastFailure,
                                    lastMerge,
                                    lastFilesRewind,
                                ].includes(item.key)}
                                putBackKey={lastFilesRewind}
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
            {!atEnd && <ToLatest onPress={toLatest} />}
            {quotable && <QuoteButton frame={frame} scroller={scroller} />}
        </div>
    );
}

/**
 * The way back to the latest message: a round button over the transcript's bottom edge, which
 * clears the message box and the queue above it however tall they are. It is there while the
 * view is scrolled up, and shows while the pointer is over the transcript or it has the
 * keyboard (`app.css`).
 */
function ToLatest({ onPress }: { onPress: () => void }) {
    return (
        <button
            type="button"
            className="round-button to-latest"
            aria-label="Scroll to the latest message"
            title="Scroll to the latest message"
            onClick={onPress}
        >
            <svg viewBox="0 0 16 16" aria-hidden="true">
                <path d="M8 3.5v9M4 8.5l4 4 4-4" />
            </svg>
        </button>
    );
}

/**
 * What the user wrote: words they quoted from a reply as a quote, with what they wrote about
 * them around it, anything they pasted large folded away under a line that says how much there
 * is of it, and the sessions, pull requests and issues it mentions where their tokens stand.
 */
function Said({ text, mentioned }: { text: string; mentioned: Mentions }) {
    return messageParts(text).map((part, index) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: the parts of a sent message never change
        <SaidPart key={index} part={part} mentioned={mentioned} />
    ));
}

/** What a message mentions, listed under it. */
interface Mentions {
    sessions: Mentioned[];
    github: GitHubMentioned[];
    elements: ListedElement[];
    /** The session the message went to, whose folder holds the elements' pictures. */
    sessionId: number;
}

function SaidPart({ part, mentioned }: { part: MessagePart; mentioned: Mentions }) {
    switch (part.kind) {
        case "text":
            return (
                <WithMentions
                    text={part.text}
                    sessions={mentioned.sessions}
                    github={mentioned.github}
                    elements={mentioned.elements}
                    sessionId={mentioned.sessionId}
                />
            );
        case "quote":
            return <blockquote className="said-quote">{part.text}</blockquote>;
        case "paste":
            return (
                <details className="pasted">
                    <summary>
                        Pasted text, {part.lines} {part.lines === 1 ? "line" : "lines"}
                    </summary>
                    <pre>{part.text}</pre>
                </details>
            );
    }
}

interface ItemProps {
    item: Item;
    sessionId: number;
    /**
     * Whether this is the conversation's latest move of the worktree, or the line of its latest
     * turn a usage limit refused, with nothing written since: only those offer something.
     */
    latest: boolean;
    /** The session whose calls' work can be sent to the background now; `null` for none. */
    sendsFrom: number | null;
    /** The rewind whose files can be put back, by its key; `null` for none. */
    putBackKey: number | null;
    /** Whether it is in a part a rewind took back, which offers nothing. */
    folded?: boolean;
}

function ItemView(props: ItemProps) {
    const { item, sessionId, latest, sendsFrom, putBackKey, folded = false } = props;
    const words = useWords();
    switch (item.kind) {
        case "user":
            return <UserSaid item={item} sessionId={sessionId} folded={folded} />;
        case "rewound":
            return <RewoundLine sessionId={sessionId} item={item} putBackKey={putBackKey} />;
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
                    sessionId={sessionId}
                    sendsFrom={sendsFrom}
                />
            );
        case "tools":
            return <ToolRun calls={item.calls} sessionId={sessionId} sendsFrom={sendsFrom} />;
        case "last-image":
            return <LastImage sessionId={sessionId} path={item.path} />;
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
        case "pull":
            return <PullLine sessionId={sessionId} item={item} latest={latest} />;
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
        case "limit":
            return <LimitLine sessionId={sessionId} item={item} latest={latest} />;
    }
}

/**
 * What the user wrote, with the ways to go on from it: a fork from here, and taking the session
 * back to before it to edit it. A message a rewind took back offers neither.
 */
function UserSaid(props: {
    item: Extract<Item, { kind: "user" }>;
    sessionId: number;
    folded: boolean;
}) {
    const { item, sessionId, folded } = props;
    const words = useWords();
    return (
        <div className="said">
            {item.remote && <p className="said-from">Typed on claude.ai</p>}
            {item.atReset && <p className="said-from">Sent by itself when the usage limit reset</p>}
            <Said text={item.text} mentioned={{ ...item, sessionId }} />
            <MentionList
                sessions={mentionedApart(item.text, item.sessions)}
                github={githubApart(item.text, item.github)}
                elements={elementsApart(item.text, item.elements)}
                sessionId={sessionId}
            />
            <FileChips files={item.files} sessionId={sessionId} />
            {!folded && (
                <div className="said-actions">
                    <button
                        type="button"
                        className="said-fork"
                        title={words.editHereHint}
                        onClick={() => editFromMessage(sessionId, item.key)}
                    >
                        {words.editHere}
                    </button>
                    <button
                        type="button"
                        className="said-fork"
                        title={words.forkHereHint}
                        onClick={() => void forkFromMessage(sessionId, item.key)}
                    >
                        {words.forkHere}
                    </button>
                </div>
            )}
        </div>
    );
}

/**
 * Where the session was taken back to before one of its messages, with what that dropped
 * folded under it, as the transcript showed it. The latest rewind whose files went back
 * offers to put them back.
 */
function RewoundLine(props: { sessionId: number; item: RewoundItem; putBackKey: number | null }) {
    const { sessionId, item, putBackKey } = props;
    const toolCalls = useStore((state) => state.settings?.tool_calls ?? "folded");
    const turnEnds = useStore((state) => state.settings?.turn_end_lines ?? true);
    const report = useStore((state) => state.report);
    const [putting, setPutting] = useState(false);
    const dropped = useMemo(
        () => shownTools(shownLines(item.dropped, turnEnds), toolCalls),
        [item.dropped, turnEnds, toolCalls],
    );
    const messages = item.dropped.filter((one) => one.kind === "user" || one.kind === "peer");
    const commits = item.putBack ? null : commitsSaid(item.commitsSince);
    const putBack = () => {
        setPutting(true);
        api.sessionFilesPutBack(sessionId)
            .catch(report)
            .finally(() => setPutting(false));
    };
    return (
        <div className="rewound">
            <p className="asked">
                {rewoundSaid(messageWords(item.message), item.files, item.putBack)}
                {commits && ` ${commits}`}
            </p>
            {item.key === putBackKey && (
                <button
                    type="button"
                    className="button"
                    disabled={putting}
                    title="Puts the files back as they were just before they were taken back, if nothing changed them since."
                    onClick={putBack}
                >
                    Put the files back
                </button>
            )}
            {dropped.length > 0 && (
                <details className="rewound-part">
                    <summary>
                        What was taken back: {messages.length}{" "}
                        {messages.length === 1 ? "message" : "messages"} and what followed
                    </summary>
                    <ol className="items">
                        {dropped.map((inner) => (
                            <li key={inner.key} className={`item item-${inner.kind}`}>
                                <ItemView
                                    item={inner}
                                    sessionId={sessionId}
                                    latest={inner.kind === "rewound" && inner.key === putBackKey}
                                    sendsFrom={null}
                                    putBackKey={putBackKey}
                                    folded={true}
                                />
                            </li>
                        ))}
                    </ol>
                </details>
            )}
        </div>
    );
}

/**
 * The line where a usage limit stopped a turn, which says why. The latest one offers to go on
 * by itself when the limit resets, or says that it will, with the way to cancel that.
 */
function LimitLine(props: { sessionId: number; item: LimitItem; latest: boolean }) {
    const { sessionId, item, latest } = props;
    const continueAt = useStore(
        (state) => findSession(state.projects, sessionId)?.continue_at ?? null,
    );
    const report = useStore((state) => state.report);
    const [asking, setAsking] = useState(false);
    const now = new Date();
    const offer = continueOffer(item, latest, continueAt, Math.floor(now.getTime() / 1000));
    // An offer is for a reset still to come: at the reset it goes, and the core would refuse it.
    useRedrawAt(offer?.state === "offered" ? offer.at : null);
    const ask = (call: () => Promise<unknown>) => {
        setAsking(true);
        call()
            .catch(report)
            .finally(() => setAsking(false));
    };
    return (
        <div className="limit-line">
            <p className="notice-line notice-warning" role="alert">
                {item.text}
            </p>
            {offer?.state === "offered" && (
                <button
                    type="button"
                    className="button"
                    disabled={asking}
                    title="Sends “Continue where you stopped.” to the session when the limit resets. Writing to it, or Stop, cancels that."
                    onClick={() => ask(() => api.sessionContinueAtReset(sessionId))}
                >
                    Continue {resetWhen(offer.at, now)}
                </button>
            )}
            {offer?.state === "set" && (
                <p className="aside limit-continues">
                    Goes on by itself {resetWhen(offer.at, now)}.{" "}
                    <button
                        type="button"
                        className="link-button"
                        disabled={asking}
                        onClick={() => ask(() => api.sessionContinueCancel(sessionId))}
                    >
                        Cancel
                    </button>
                </p>
            )}
        </div>
    );
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
 * Says what changed on the task's pull request, in one line. The latest line that says checks
 * failed offers to put the failure into the message box, while checks of it still fail.
 */
function PullLine(props: {
    sessionId: number;
    item: Extract<Item, { kind: "pull" }>;
    latest: boolean;
}) {
    const { sessionId, item, latest } = props;
    const words = useWords();
    const projects = useStore((state) => state.projects);
    const session = findSession(projects, sessionId);
    const home = session && findSessionHome(projects, session);
    const pull = useStore((state) => (home ? (state.pulls[home.task.id] ?? null) : null));
    const failing = pull !== null && pull.number === item.number && failedChecks(pull).length > 0;
    const offered = latest && item.change.kind === "checks_failed" && failing && home;
    // Merged, the task can be put away, while it is not and that pull request is still its.
    const finished =
        latest &&
        item.change.kind === "merged" &&
        pull?.number === item.number &&
        home &&
        offersPutAway(home.task, pull);
    return (
        <div className="pull-changed">
            <p className="asked" data-change={item.change.kind}>
                {pullChangeSaid(item.number, item.change, words)}
            </p>
            {offered && (
                <SendFailure
                    project={home.project.name}
                    task={home.task.name}
                    sessionId={sessionId}
                />
            )}
            {finished && (
                <PutAwayButton
                    project={home.project.name}
                    task={home.task}
                    label={`Put this ${words.terms.task} away`}
                />
            )}
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

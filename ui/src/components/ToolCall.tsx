import { type MouseEvent, memo, useState } from "react";
import { api } from "../api";
import { shownHtml } from "../htmlview";
import { useStore } from "../store";
import {
    imagesOf,
    mayGoToBackground,
    PLAN_TOOL,
    planOf,
    sameToolCall,
    summarize,
    type ToolItem,
    type ToolShown,
    toolRunSummary,
} from "../transcript";
import { useWords } from "../useWords";
import { Copyable } from "./Copyable";
import { HtmlView } from "./HtmlView";
import { Markdown } from "./Markdown";
import { ToolImages } from "./ToolImages";

/** What a call's line says once the work it waited on was moved to the background. */
const MOVED = "moved to the background";

interface CallProps extends ToolShown {
    /** The session the call was made in, whose images it returned are read from it. */
    sessionId: number;
    /**
     * The session whose agent can send the work this call waits on to the background, while
     * it is at work; `null` where none can.
     */
    sendsFrom: number | null;
}

/** Whether two calls are drawn alike, offers to send their work to the background included. */
function sameCall(a: CallProps, b: CallProps): boolean {
    return sameToolCall(a, b) && a.sessionId === b.sessionId && a.sendsFrom === b.sendsFrom;
}

/**
 * One tool call on one line; opening it shows the arguments and the result. The images it
 * returned stand under the line, open or not. A call that has not changed is not drawn again
 * while a reply is written piece by piece below it.
 */
export const ToolCall = memo(function ToolCall(call: CallProps) {
    const { id, name, input, result, moved, sessionId, sendsFrom } = call;
    // The arguments and the result are only laid out once somebody opens the call to read them.
    const [opened, setOpened] = useState(false);
    const views = useStore((state) => state.settings?.html_views ?? false);
    if (name === PLAN_TOOL) {
        return <Plan text={planOf(input)} />;
    }
    const html = views ? shownHtml(name, input) : "";
    if (html !== "") {
        return <HtmlView html={html} />;
    }
    const state = result === null ? "running" : result.isError ? "failed" : "done";
    const offered = sendsFrom !== null && mayGoToBackground(call);
    return (
        <>
            <details
                className={`tool tool-${state}`}
                onToggle={(event) => event.currentTarget.open && setOpened(true)}
            >
                <summary>
                    <span className="tool-name">{name}</span>
                    <span className="tool-summary">{summarize(input)}</span>
                    {offered && <SendToBackground sessionId={sendsFrom} callId={id} />}
                    {state !== "done" && <span className="tool-state">{state}</span>}
                    {state === "done" && moved && <span className="tool-state">{MOVED}</span>}
                </summary>
                {opened && <ToolBody text={JSON.stringify(input, null, 2)} />}
                {opened && result && <ToolBody text={result.content} />}
            </details>
            <ToolImages sessionId={sessionId} paths={result?.images ?? []} />
        </>
    );
}, sameCall);

/**
 * A call's input or result, opened, with a button that copies it as shown. A result with no
 * output says so, and has nothing to copy.
 */
function ToolBody({ text }: { text: string }) {
    if (text === "") {
        return <pre className="tool-body">(no output)</pre>;
    }
    return (
        <Copyable text={text} className="tool-copyable">
            <pre className="tool-body">{text}</pre>
        </Copyable>
    );
}

/** What is said where the work of a call did not go to the background, as asked. */
const NOT_MOVED = "not sent: it ended, or is there already";

/**
 * The button that sends the work a running call waits on to the background, as Ctrl+B does
 * in Claude Code's terminal. Once it went, the call's result and its line say so; where it did
 * not, a quiet note says why in the button's place.
 */
function SendToBackground({ sessionId, callId }: { sessionId: number; callId: string }) {
    const [asked, setAsked] = useState(false);
    const [said, setSaid] = useState<string | null>(null);
    if (said !== null) {
        return (
            <span className="tool-note" title={said}>
                {said}
            </span>
        );
    }
    const send = (event: MouseEvent) => {
        // It sits in the line that opens the call, which pressing it must not open.
        event.preventDefault();
        setAsked(true);
        api.sessionSendToBackground(sessionId, callId).then(
            (moved) => setSaid(moved ? null : NOT_MOVED),
            (error: unknown) => setSaid(String(error)),
        );
    };
    return (
        <button
            type="button"
            className="tool-send-background"
            title="The call returns now and the turn goes on; the work runs on in the background"
            disabled={asked}
            onClick={send}
        >
            {asked ? "Sending…" : "Send to background"}
        </button>
    );
}

type Run = [ToolItem, ...ToolItem[]];

interface RunProps {
    calls: Run;
    sessionId: number;
    /** As for [`ToolCall`]: offered for the last call, which the line names. */
    sendsFrom: number | null;
}

/** Whether two runs of folded calls are drawn alike: the same calls, each drawn alike. */
function sameRun(a: RunProps, b: RunProps): boolean {
    return (
        a.sessionId === b.sessionId &&
        a.sendsFrom === b.sendsFrom &&
        a.calls.length === b.calls.length &&
        a.calls.every((call, at) => {
            const other = b.calls[at];
            return other !== undefined && sameToolCall(call, other);
        })
    );
}

/**
 * Tool calls that followed each other, folded into one line that counts them and names the
 * last one used. The images they returned stay in view under it, folded or not; opening it
 * lists the calls as they show on their own, each with its own. While the agent works, the
 * line counts up as calls join it.
 */
export const ToolRun = memo(function ToolRun({ calls, sessionId, sendsFrom }: RunProps) {
    // The calls are only laid out once somebody opens the run to read them.
    const [opened, setOpened] = useState(false);
    const [open, setOpen] = useState(false);
    const { count, last, failed, running } = toolRunSummary(calls);
    const state = running ? "running" : failed > 0 ? "failed" : "done";
    const offered = sendsFrom !== null && mayGoToBackground(last);
    const toggled = (now: boolean) => {
        setOpen(now);
        setOpened((before) => before || now);
    };
    return (
        <>
            <details
                className={`tool tool-run tool-${state}`}
                onToggle={(event) => toggled(event.currentTarget.open)}
            >
                <summary>
                    <span className="tool-run-count">
                        {count === 1 ? "1 tool call" : `${count} tool calls`}
                    </span>
                    <span className="tool-name">{last.name}</span>
                    <span className="tool-summary">{summarize(last.input)}</span>
                    {offered && <SendToBackground sessionId={sendsFrom} callId={last.id} />}
                    {failed > 0 && (
                        <span className="tool-state tool-run-failed">{failed} failed</span>
                    )}
                    {running && <span className="tool-state">running</span>}
                    {!running && last.moved && <span className="tool-state">{MOVED}</span>}
                </summary>
                {opened && (
                    <ol className="tool-run-calls">
                        {calls.map((call) => (
                            <li key={call.key}>
                                {/* Calls made at once each wait on work of their own, so each
                                offers it; the line above already does for the last. */}
                                <ToolCall
                                    id={call.id}
                                    name={call.name}
                                    input={call.input}
                                    result={call.result}
                                    moved={call.moved}
                                    sessionId={sessionId}
                                    sendsFrom={offered && call === last ? null : sendsFrom}
                                />
                            </li>
                        ))}
                    </ol>
                )}
            </details>
            {/* Opened, each call shows its own. */}
            {!open && <ToolImages sessionId={sessionId} paths={imagesOf(calls)} />}
        </>
    );
}, sameRun);

/** A plan the agent presents for approval, read as prose instead of as a tool call's arguments. */
function Plan({ text }: { text: string }) {
    const words = useWords();
    return (
        <section className="plan" aria-label="Plan">
            <p className="plan-title">Plan</p>
            <Markdown text={text || `The ${words.terms.agent} gave no plan.`} />
        </section>
    );
}

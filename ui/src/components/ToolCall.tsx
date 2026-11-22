import { memo, useState } from "react";
import { shownHtml } from "../htmlview";
import { useStore } from "../store";
import {
    PLAN_TOOL,
    planOf,
    sameToolCall,
    summarize,
    type ToolItem,
    type ToolShown,
    toolRunSummary,
} from "../transcript";
import { useWords } from "../useWords";
import { HtmlView } from "./HtmlView";
import { Markdown } from "./Markdown";

/**
 * One tool call on one line; opening it shows the arguments and the result. A call that has
 * not changed is not drawn again while a reply is written piece by piece below it.
 */
export const ToolCall = memo(function ToolCall({ name, input, result }: ToolShown) {
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
    return (
        <details
            className={`tool tool-${state}`}
            onToggle={(event) => event.currentTarget.open && setOpened(true)}
        >
            <summary>
                <span className="tool-name">{name}</span>
                <span className="tool-summary">{summarize(input)}</span>
                {state !== "done" && <span className="tool-state">{state}</span>}
            </summary>
            {opened && <pre className="tool-body">{JSON.stringify(input, null, 2)}</pre>}
            {opened && result && <pre className="tool-body">{result.content || "(no output)"}</pre>}
        </details>
    );
}, sameToolCall);

type Run = [ToolItem, ...ToolItem[]];

/** Whether two runs of folded calls are drawn alike: the same calls, each drawn alike. */
function sameRun(a: { calls: Run }, b: { calls: Run }): boolean {
    return (
        a.calls.length === b.calls.length &&
        a.calls.every((call, at) => {
            const other = b.calls[at];
            return other !== undefined && sameToolCall(call, other);
        })
    );
}

/**
 * Tool calls that followed each other, folded into one line that counts them and names the
 * last one used. Opening it lists the calls as they show on their own. While the agent works,
 * the line counts up as calls join it.
 */
export const ToolRun = memo(function ToolRun({ calls }: { calls: Run }) {
    // The calls are only laid out once somebody opens the run to read them.
    const [opened, setOpened] = useState(false);
    const { count, last, failed, running } = toolRunSummary(calls);
    const state = running ? "running" : failed > 0 ? "failed" : "done";
    return (
        <details
            className={`tool tool-run tool-${state}`}
            onToggle={(event) => event.currentTarget.open && setOpened(true)}
        >
            <summary>
                <span className="tool-run-count">
                    {count === 1 ? "1 tool call" : `${count} tool calls`}
                </span>
                <span className="tool-name">{last.name}</span>
                <span className="tool-summary">{summarize(last.input)}</span>
                {failed > 0 && <span className="tool-state tool-run-failed">{failed} failed</span>}
                {running && <span className="tool-state">running</span>}
            </summary>
            {opened && (
                <ol className="tool-run-calls">
                    {calls.map((call) => (
                        <li key={call.key}>
                            <ToolCall name={call.name} input={call.input} result={call.result} />
                        </li>
                    ))}
                </ol>
            )}
        </details>
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

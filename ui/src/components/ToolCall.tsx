import { memo, useState } from "react";
import { shownHtml } from "../htmlview";
import { useStore } from "../store";
import { PLAN_TOOL, planOf, sameToolCall, summarize, type ToolShown } from "../transcript";
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

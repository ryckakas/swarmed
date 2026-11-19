import { memo } from "react";
import { htmlOf, isShowHtmlTool } from "../htmlview";
import { useStore } from "../store";
import { PLAN_TOOL, planOf, summarize, type ToolResult } from "../transcript";
import { HtmlView } from "./HtmlView";
import { Markdown } from "./Markdown";

interface Props {
    name: string;
    input: unknown;
    result: ToolResult | null;
}

/** One tool call on one line; opening it shows the arguments and the result. */
export const ToolCall = memo(function ToolCall({ name, input, result }: Props) {
    const views = useStore((state) => state.settings?.html_views ?? false);
    if (name === PLAN_TOOL) {
        return <Plan text={planOf(input)} />;
    }
    if (views && isShowHtmlTool(name) && htmlOf(input) !== "") {
        return <HtmlView html={htmlOf(input)} />;
    }
    const state = result === null ? "running" : result.isError ? "failed" : "done";
    return (
        <details className={`tool tool-${state}`}>
            <summary>
                <span className="tool-name">{name}</span>
                <span className="tool-summary">{summarize(input)}</span>
                {state !== "done" && <span className="tool-state">{state}</span>}
            </summary>
            <pre className="tool-body">{JSON.stringify(input, null, 2)}</pre>
            {result && <pre className="tool-body">{result.content || "(no output)"}</pre>}
        </details>
    );
});

/** A plan the agent presents for approval, read as prose instead of as a tool call's arguments. */
function Plan({ text }: { text: string }) {
    return (
        <section className="plan" aria-label="Plan">
            <p className="plan-title">Plan</p>
            <Markdown text={text || "The agent gave no plan."} />
        </section>
    );
}

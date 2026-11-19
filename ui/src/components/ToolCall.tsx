import { memo } from "react";
import { summarize, type ToolResult } from "../transcript";

interface Props {
    name: string;
    input: unknown;
    result: ToolResult | null;
}

/** One tool call on one line; opening it shows the arguments and the result. */
export const ToolCall = memo(function ToolCall({ name, input, result }: Props) {
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

import { useLayoutEffect, useMemo, useRef } from "react";
import type { AgentEvent } from "../api/types";
import { type Item, summarize, toItems } from "../transcript";
import { Markdown } from "./Markdown";
import { PermissionPrompt } from "./PermissionPrompt";
import { ToolCall } from "./ToolCall";

/** How close to the bottom, in pixels, still counts as "following the conversation". */
const FOLLOW_DISTANCE = 80;

export function Transcript({ sessionId, events }: { sessionId: number; events: AgentEvent[] }) {
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
            </ol>
        </div>
    );
}

function ItemView({ item, sessionId }: { item: Item; sessionId: number }) {
    switch (item.kind) {
        case "user":
            return <p className="said">{item.text}</p>;
        case "text":
            return <Markdown text={item.text} />;
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
        case "denied":
            return (
                <p className="asked">
                    You denied {item.toolName}:{" "}
                    <span className="mono">{summarize(item.input)}</span>
                </p>
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
                    {item.isError ? "Turn stopped" : "Turn finished"}
                    {item.costUsd !== null && `, $${item.costUsd.toFixed(2)} so far`}
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

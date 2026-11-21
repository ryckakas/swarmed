// Sub-agents: agents a session's own agent starts for a part of its work. They come and go by
// themselves, so everything here is read from the session's events. Pure, so it can be tested.

import type { AgentEvent } from "./api/types";

/** The tools Claude Code starts a sub-agent with: the name changed between its versions. */
const STARTING_TOOLS = ["Agent", "Task"];

export interface SubAgent {
    /** The tool call that started it: what its events are filed under. */
    id: string;
    /** What it is to do, in a few words of the agent's. */
    name: string;
    /** The kind of agent, as the agent names it: `Explore`, `general-purpose`. */
    kind: string;
    /** What it was asked, in full. */
    prompt: string;
}

function text(value: unknown): string {
    return typeof value === "string" ? value : "";
}

/** The sub-agent a tool call starts, if it starts one. */
function started(event: AgentEvent): SubAgent | null {
    if (event.type !== "tool_use" || !STARTING_TOOLS.includes(event.name)) {
        return null;
    }
    const input = (event.input ?? {}) as Record<string, unknown>;
    const kind = text(input.subagent_type);
    return {
        id: event.id,
        name: text(input.description) || kind || "Sub-agent",
        kind,
        prompt: text(input.prompt),
    };
}

/**
 * The sub-agents at work after `event`, given the ones at work before it. One starts with
 * the tool call for it and is done when that call has its result; none outlasts the turn or
 * the agent itself.
 */
export function atWork(running: SubAgent[], event: AgentEvent): SubAgent[] {
    switch (event.type) {
        case "tool_use": {
            const one = started(event);
            return one ? [...running.filter((other) => other.id !== one.id), one] : running;
        }
        case "tool_result":
            return running.some((one) => one.id === event.tool_use_id)
                ? running.filter((one) => one.id !== event.tool_use_id)
                : running;
        case "turn_done":
        case "exited":
            return running.length === 0 ? running : [];
        default:
            return running;
    }
}

/** The sub-agents still at work at the end of `events`. */
export function atWorkIn(events: AgentEvent[]): SubAgent[] {
    return events.reduce(atWork, []);
}

/** The sub-agent that tool call `id` started, at work or done; nothing if it started none. */
export function subAgentOf(events: AgentEvent[], id: string): SubAgent | null {
    const call = events.find((event) => event.type === "tool_use" && event.id === id);
    return call ? started(call) : null;
}

/** What sub-agent `id` said and did, as events of a conversation of its own. */
export function eventsOfSubAgent(events: AgentEvent[], id: string): AgentEvent[] {
    return events.flatMap((event) =>
        event.type === "sub" && event.parent === id ? [event.event] : [],
    );
}

/** What sub-agent `id` handed back when it was done, or nothing while it is at work. */
export function answerOfSubAgent(events: AgentEvent[], id: string): string | null {
    const result = events.find((event) => event.type === "tool_result" && event.tool_use_id === id);
    return result?.type === "tool_result" ? result.content : null;
}

// Sub-agents: agents a session's own agent starts for a part of its work. They come and go by
// themselves, so everything here is read from the session's events. Pure, so it can be tested.

import type { AgentEvent, SessionStatus } from "./api/types";

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
    /** Whether its call has its result. One in the background gets it at once. */
    answered: boolean;
    /** Whether it runs in the background, as its agent reported: it outlives its call. */
    background: boolean;
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
        // Nothing to name it by: the window calls it what the style calls a sub-agent.
        name: text(input.description) || kind,
        kind,
        prompt: text(input.prompt),
        answered: false,
        background: false,
    };
}

/** `followed` with sub-agent `id` changed, or as it was if it holds no such sub-agent. */
function changed(followed: SubAgent[], id: string, change: (one: SubAgent) => SubAgent) {
    return followed.some((one) => one.id === id)
        ? followed.map((one) => (one.id === id ? change(one) : one))
        : followed;
}

/** What a report of something the agent left running changes for the sub-agent it is. */
function reported(followed: SubAgent[], event: Extract<AgentEvent, { type: "background" }>) {
    const id = event.tool_use_id;
    if (id === null) {
        return followed;
    }
    if (event.state === "running") {
        return changed(followed, id, (one) => ({ ...one, background: true }));
    }
    return followed.some((one) => one.id === id)
        ? followed.filter((one) => one.id !== id)
        : followed;
}

/**
 * The sub-agents followed after `event`, given the ones followed before it. One starts with
 * the tool call for it. In the foreground it is done when that call has its result; in the
 * background, which its agent reports and which may come before or after that result, when
 * its agent reports its end. A turn's end leaves only those in the background, and the
 * agent's end leaves none. Whether one is at work is [`working`].
 */
export function atWork(followed: SubAgent[], event: AgentEvent): SubAgent[] {
    switch (event.type) {
        case "tool_use": {
            const one = started(event);
            return one ? [...followed.filter((other) => other.id !== one.id), one] : followed;
        }
        case "tool_result":
            return changed(followed, event.tool_use_id, (one) => ({ ...one, answered: true }));
        case "background":
            return reported(followed, event);
        case "turn_done": {
            const kept = followed.filter((one) => one.background);
            return kept.length === followed.length ? followed : kept;
        }
        case "exited":
            return followed.length === 0 ? followed : [];
        default:
            return followed;
    }
}

/** Whether a followed sub-agent is at work: its call has no result yet, or it runs in the background. */
export function working(one: SubAgent): boolean {
    return !one.answered || one.background;
}

/** The sub-agents followed at the end of `events`, to go on from with [`atWork`]. */
export function subAgentsIn(events: AgentEvent[]): SubAgent[] {
    return events.reduce(atWork, []);
}

/** The sub-agents at work at the end of `events`. */
export function atWorkIn(events: AgentEvent[]): SubAgent[] {
    return subAgentsIn(events).filter(working);
}

/**
 * The sub-agents to list under a session. While its agent works, all at work; while it is
 * idle, those in the background, which outlive the turn; none once it has failed.
 */
export function listedUnder(followed: SubAgent[], status: SessionStatus): SubAgent[] {
    if (status === "errored") {
        return [];
    }
    const idle = status === "idle";
    return followed.filter((one) => working(one) && (one.background || !idle));
}

/** Whether sub-agent `id` is at work, in a session whose status is `status`. */
export function isAtWork(events: AgentEvent[], id: string, status: SessionStatus): boolean {
    return listedUnder(subAgentsIn(events), status).some((one) => one.id === id);
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

/**
 * What sub-agent `id` handed back when it was done, or nothing while it is at work. One in the
 * background is answered at once with only that it started: its end report says how it went.
 */
export function answerOfSubAgent(events: AgentEvent[], id: string): string | null {
    const reports = events.filter(
        (event) => event.type === "background" && event.tool_use_id === id,
    );
    if (reports.length > 0) {
        const last = reports[reports.length - 1];
        return last?.type === "background" && last.state !== "running" ? last.summary : null;
    }
    const result = events.find((event) => event.type === "tool_result" && event.tool_use_id === id);
    return result?.type === "tool_result" ? result.content : null;
}

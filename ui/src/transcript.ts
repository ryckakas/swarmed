import type { AgentEvent } from "./api/types";
import { isShowHtmlTool } from "./htmlview";

// Turns a session's raw events into what the transcript shows. Pure, so it can be tested.

export type Item =
    | { kind: "user"; key: number; text: string }
    /** Something the agent said. `streaming` while its end has not arrived yet. */
    | { kind: "text"; key: number; text: string; streaming: boolean }
    | { kind: "tool"; key: number; name: string; input: unknown; result: ToolResult | null }
    /** A request nobody has decided yet. It leaves the transcript once it is. */
    | {
          kind: "permission";
          key: number;
          requestId: string;
          toolName: string;
          input: unknown;
          description: string | null;
      }
    /** A request the user refused. An allowed one shows as the tool call it let through. */
    | { kind: "denied"; key: number; toolName: string; input: unknown }
    /** What happened in the terminal, as the agent was told with the message before it. */
    | { kind: "terminal"; key: number; text: string }
    | { kind: "session"; key: number; model: string; login: string; mcp: string; skills: number }
    | { kind: "turn"; key: number; isError: boolean; costUsd: number | null }
    | { kind: "notice"; key: number; tone: "error" | "warning"; text: string };

export interface ToolResult {
    content: string;
    isError: boolean;
}

/** The tool an agent in plan mode asks to use when its plan is ready. Allowing it approves the plan. */
export const PLAN_TOOL = "ExitPlanMode";

/** The plan an agent presents with [`PLAN_TOOL`], as Markdown; empty if it gave none. */
export function planOf(input: unknown): string {
    const plan =
        typeof input === "object" && input !== null ? (input as { plan?: unknown }).plan : "";
    return typeof plan === "string" ? plan : "";
}

/** How long a tool call's arguments may be when shown on one line. */
const SUMMARY_CHARS = 120;
const TELLING_ARGUMENTS = ["command", "file_path", "pattern", "url", "query"];

/** The one argument that says what a tool call does, or the arguments as compact JSON. */
export function summarize(input: unknown): string {
    const record = typeof input === "object" && input !== null ? input : {};
    const named = TELLING_ARGUMENTS.map((key) => (record as Record<string, unknown>)[key]).find(
        (value) => typeof value === "string",
    );
    const text = typeof named === "string" ? named : JSON.stringify(input);
    return text.length <= SUMMARY_CHARS ? text : `${text.slice(0, SUMMARY_CHARS - 1)}…`;
}

/**
 * Events after which no earlier permission request can still be open. A history written before
 * answers were recorded has only these to go by.
 */
const CLOSES_REQUESTS = new Set<AgentEvent["type"]>(["user_message", "turn_done", "exited"]);

function single(event: AgentEvent, key: number): Item | null {
    switch (event.type) {
        case "user_message":
            return { kind: "user", key, text: event.text };
        case "text":
            return { kind: "text", key, text: event.text, streaming: false };
        // A reply still being written reads like a finished one, and becomes one in place.
        case "text_delta":
            return { kind: "text", key, text: event.text, streaming: true };
        case "terminal_shared":
            return { kind: "terminal", key, text: event.text };
        case "init":
            return sessionItem(event, key);
        case "turn_done":
            return { kind: "turn", key, isError: event.is_error, costUsd: event.cost_usd };
        case "rate_limit":
            return rateLimitNotice(event, key);
        case "exited":
            return exitNotice(event, key);
        default:
            return null;
    }
}

function sessionItem(event: Extract<AgentEvent, { type: "init" }>, key: number): Item {
    const count = (status: string) =>
        event.mcp_servers.filter((server) => server.status === status).length;
    // Claude Code says which servers connected. Other agents are only known to have been given them.
    const passed = count("passed");
    const mcp =
        passed > 0
            ? `${passed} MCP servers passed to the agent`
            : `${count("connected")} of ${event.mcp_servers.length} MCP servers connected`;
    return {
        kind: "session",
        key,
        model: event.model,
        login: event.api_key_source === "none" ? "subscription" : event.api_key_source,
        mcp,
        skills: event.skills,
    };
}

function rateLimitNotice(event: Extract<AgentEvent, { type: "rate_limit" }>, key: number) {
    if (event.status === "allowed") {
        return null;
    }
    const resets =
        event.resets_at === null
            ? ""
            : ` It resets at ${new Date(event.resets_at * 1000).toLocaleTimeString()}.`;
    const text = `Usage limit reached (${event.status}).${resets}`;
    return { kind: "notice", key, tone: "warning", text } satisfies Item;
}

function exitNotice(event: Extract<AgentEvent, { type: "exited" }>, key: number) {
    if (event.code === null || event.code === 0) {
        return null;
    }
    const detail = event.stderr.trim();
    const text = `The agent stopped with exit code ${event.code}.${detail ? `\n${detail}` : ""}`;
    return { kind: "notice", key, tone: "error", text } satisfies Item;
}

type Permission = Extract<Item, { kind: "permission" }>;

/** What is being built up while walking through a session's events. */
interface Draft {
    items: Item[];
    tools: Map<string, Extract<Item, { kind: "tool" }>>;
    /** The permission requests still waiting for a decision, by request id. */
    open: Map<string, Permission>;
    /** Requests that were decided or abandoned; they are left out of the result. */
    settled: Set<Item>;
}

/** The transcript items for a session's events, in order. Tool results join their calls. */
export function toItems(events: AgentEvent[]): Item[] {
    const draft: Draft = { items: [], tools: new Map(), open: new Map(), settled: new Set() };
    events.forEach((event, key) => {
        add(draft, event, key);
    });
    return draft.items.filter((item) => !draft.settled.has(item));
}

function add(draft: Draft, event: AgentEvent, key: number) {
    if (CLOSES_REQUESTS.has(event.type)) {
        for (const request of draft.open.values()) {
            draft.settled.add(request);
        }
        draft.open.clear();
    }
    switch (event.type) {
        case "tool_use":
            return addTool(draft, event, key);
        case "tool_result":
            return attachResult(draft.tools.get(event.tool_use_id), event);
        case "permission_request":
            return addRequest(draft, event, key);
        case "permission_answered":
            return settle(draft, event.request_id, key, event.allowed);
        case "permission_withdrawn":
            return settle(draft, event.request_id, key, true);
        default:
            return addSingle(draft, event, key);
    }
}

function addSingle(draft: Draft, event: AgentEvent, key: number) {
    const item = single(event, key);
    if (item) {
        draft.items.push(item);
    }
}

function addTool(draft: Draft, event: Extract<AgentEvent, { type: "tool_use" }>, key: number) {
    const tool: Item = { kind: "tool", key, name: event.name, input: event.input, result: null };
    draft.tools.set(event.id, tool);
    draft.items.push(tool);
}

function attachResult(
    tool: Extract<Item, { kind: "tool" }> | undefined,
    event: Extract<AgentEvent, { type: "tool_result" }>,
) {
    if (tool) {
        tool.result = { content: event.content, isError: event.is_error };
    }
}

function addRequest(
    draft: Draft,
    event: Extract<AgentEvent, { type: "permission_request" }>,
    key: number,
) {
    const request: Permission = {
        kind: "permission",
        key,
        requestId: event.request_id,
        toolName: event.tool_name,
        input: event.input,
        description: event.description,
    };
    draft.open.set(event.request_id, request);
    draft.items.push(request);
}

/** Takes a decided request out of the transcript; a refusal leaves a line where it happened. */
function settle(draft: Draft, requestId: string, key: number, allowed: boolean) {
    const request = draft.open.get(requestId);
    if (!request) {
        return;
    }
    draft.open.delete(requestId);
    draft.settled.add(request);
    if (!allowed) {
        const { toolName, input } = request;
        draft.items.push({ kind: "denied", key, toolName, input });
    }
}

/**
 * What a working agent is doing right now, in a word or two, from the events of the turn so far.
 * It fills the silence between sending a message and the first thing the agent shows.
 */
export function activity(events: AgentEvent[]): string {
    const start = events.findLastIndex((event) => event.type === "user_message");
    const turn = events.slice(start + 1);
    const last = turn.at(-1);
    if (last === undefined || last.type === "init") {
        return "Starting";
    }
    if (last.type === "text_delta") {
        return "Writing";
    }
    const finished = new Set(
        turn.flatMap((event) => (event.type === "tool_result" ? [event.tool_use_id] : [])),
    );
    const running = turn.findLast((event) => event.type === "tool_use" && !finished.has(event.id));
    if (running?.type !== "tool_use") {
        return "Thinking";
    }
    if (isShowHtmlTool(running.name)) {
        return "Showing a view";
    }
    return running.name === PLAN_TOOL ? "Presenting its plan" : `Running ${running.name}`;
}

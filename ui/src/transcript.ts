import type { AgentEvent } from "./api/types";

// Turns a session's raw events into what the transcript shows. Pure, so it can be tested.

export type Item =
    | { kind: "user"; key: number; text: string }
    | { kind: "text"; key: number; text: string }
    | { kind: "tool"; key: number; name: string; input: unknown; result: ToolResult | null }
    | {
          kind: "permission";
          key: number;
          requestId: string;
          toolName: string;
          input: unknown;
          description: string | null;
          /** Still waiting for a decision. */
          pending: boolean;
      }
    | { kind: "session"; key: number; model: string; login: string; mcp: string; skills: number }
    | { kind: "turn"; key: number; isError: boolean; costUsd: number | null }
    | { kind: "notice"; key: number; tone: "error" | "warning"; text: string };

export interface ToolResult {
    content: string;
    isError: boolean;
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

/** Events that show the agent moved on, so an earlier permission request was answered. */
const ANSWERS_A_REQUEST = new Set<AgentEvent["type"]>(["tool_result", "turn_done", "exited"]);

function single(event: AgentEvent, key: number): Item | null {
    switch (event.type) {
        case "user_message":
            return { kind: "user", key, text: event.text };
        case "text":
            return { kind: "text", key, text: event.text };
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

/** The transcript items for a session's events, in order. Tool results join their calls. */
export function toItems(events: AgentEvent[]): Item[] {
    const items: Item[] = [];
    const tools = new Map<string, Extract<Item, { kind: "tool" }>>();
    events.forEach((event, key) => {
        if (event.type === "tool_use") {
            const tool: Item = {
                kind: "tool",
                key,
                name: event.name,
                input: event.input,
                result: null,
            };
            tools.set(event.id, tool);
            items.push(tool);
        } else if (event.type === "tool_result") {
            attachResult(tools.get(event.tool_use_id), event);
        } else if (event.type === "permission_request") {
            items.push(permissionItem(event, key, events));
        } else {
            const item = single(event, key);
            if (item) {
                items.push(item);
            }
        }
    });
    return items;
}

function attachResult(
    tool: Extract<Item, { kind: "tool" }> | undefined,
    event: Extract<AgentEvent, { type: "tool_result" }>,
) {
    if (tool) {
        tool.result = { content: event.content, isError: event.is_error };
    }
}

function permissionItem(
    event: Extract<AgentEvent, { type: "permission_request" }>,
    key: number,
    events: AgentEvent[],
): Item {
    const answered = events.slice(key + 1).some((later) => ANSWERS_A_REQUEST.has(later.type));
    return {
        kind: "permission",
        key,
        requestId: event.request_id,
        toolName: event.tool_name,
        input: event.input,
        description: event.description,
        pending: !answered,
    };
}

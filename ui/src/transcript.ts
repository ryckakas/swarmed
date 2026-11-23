import type { AgentEvent, ScreenSection, ToolCalls } from "./api/types";
import { shownHtml } from "./htmlview";
import { QUESTION_TOOL, type Question, questionsOf } from "./questions";
import { limitNotice, type RateLimit } from "./rateLimit";
import { type Words, wordsFor } from "./wording";

// Turns a session's raw events into what the transcript shows. Pure, so it can be tested.

export type Item =
    /**
     * What the user wrote, and the files they pointed the agent at with it; `remote` for what
     * they typed on claude.ai or in the Claude app, which reached the session from there.
     */
    | { kind: "user"; key: number; text: string; files: string[]; remote: boolean }
    /** A message another of the user's sessions left for this one. */
    | { kind: "peer"; key: number; from: number; title: string; text: string }
    /** Something the agent said. `streaming` while its end has not arrived yet. */
    | { kind: "text"; key: number; text: string; streaming: boolean }
    | ToolItem
    /** Tool calls that followed each other, folded into one line (`shownTools`). */
    | { kind: "tools"; key: number; calls: [ToolItem, ...ToolItem[]] }
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
    /** A change to Swarmed's configuration the agent proposed; `accepted` once it is answered. */
    | {
          kind: "proposal";
          key: number;
          requestId: string;
          title: string;
          detail: string;
          accepted: boolean | null;
      }
    /** One of the agent's own screens, such as `/usage`, which the user asked to see. */
    | {
          kind: "screen";
          key: number;
          command: string;
          title: string;
          note: string;
          sections: ScreenSection[];
      }
    /**
     * What a command such as `/model` asks the user. It stays where it was asked: `said` is
     * what came of it once it is answered, and `null` while it waits.
     */
    | {
          kind: "asked";
          key: number;
          requestId: string;
          command: string;
          questions: Question[];
          said: string | null;
      }
    /** A command the agent could only show on a screen in its own terminal. */
    | { kind: "terminal-only"; key: number; command: string }
    /**
     * Remote Control was turned on, with where the session is on claude.ai, or off, or it
     * failed. Reconnecting and waiting for the agent are shown in the header alone.
     */
    | {
          kind: "remote";
          key: number;
          state: "on" | "off" | "failed";
          url: string | null;
          detail: string | null;
      }
    /** The task's worktree moved to another branch: during the turn, or since the last one. */
    | { kind: "moved"; key: number; from: string | null; to: string | null; betweenTurns: boolean }
    /** HTML the agent showed the user through Swarmed's tool. */
    | { kind: "view"; key: number; html: string }
    /** What happened in the terminal, as the agent was told with the message before it. */
    | { kind: "terminal"; key: number; text: string }
    | { kind: "session"; key: number; model: string; login: string; mcp: string; skills: number }
    | {
          kind: "turn";
          key: number;
          isError: boolean;
          costUsd: number | null;
          /** How long the turn took, if it was measured. */
          durationMs: number | null;
      }
    | { kind: "notice"; key: number; tone: "error" | "warning"; text: string }
    /** Where the copy of an earlier conversation ends and the fork's own begins. */
    | { kind: "forked"; key: number; from: number };

export interface ToolResult {
    content: string;
    isError: boolean;
}

/** One tool call, as the transcript holds it. */
export type ToolItem = {
    kind: "tool";
    key: number;
    name: string;
    input: unknown;
    result: ToolResult | null;
};

/** The line that opens the list of attached files in a message, as the core writes it. */
const ATTACHED_FILES = "Attached files:";

/** A message with the files attached to it listed underneath, as the agent is sent it. */
export function withFiles(text: string, files: string[]): string {
    const list = [ATTACHED_FILES, ...files.map((file) => `- ${file}`)].join("\n");
    if (files.length === 0) {
        return text.trim();
    }
    return text.trim() === "" ? list : `${text.trim()}\n\n${list}`;
}

/** A message taken apart again: what the user wrote, and the files listed under it. */
export function splitFiles(message: string): { text: string; files: string[] } {
    const lines = message.split("\n");
    const at = lines.lastIndexOf(ATTACHED_FILES);
    const listed = at === -1 ? [] : lines.slice(at + 1);
    // Only a list that runs to the end of the message, and is nothing but files, is one.
    const isList = listed.length > 0 && listed.every((line) => line.startsWith("- "));
    if (!isList || (at > 0 && lines[at - 1] !== "")) {
        return { text: message, files: [] };
    }
    const text = lines.slice(0, Math.max(at - 1, 0)).join("\n");
    return { text, files: listed.map((line) => line.slice(2)) };
}

/** The last part of a path: the name a file is known by. */
export function fileName(path: string): string {
    return path.replace(/\/+$/, "").split("/").pop() || path;
}

/** How the core marks a message one session left for another. */
const FROM_SESSION =
    /^Message from session (\d+) \("([^"]*)"\), another agent session of the same user:\n\n([\s\S]*)$/;

/** Whether a message was left by another session, rather than typed by the user. */
export function isFromSession(text: string): boolean {
    return FROM_SESSION.test(text);
}

/** A message as the transcript shows it: the user's own, or one another session left. */
function messageItem(text: string, key: number, remote = false): Item {
    const left = FROM_SESSION.exec(text);
    if (!left) {
        return { kind: "user", key, ...splitFiles(text), remote };
    }
    return { kind: "peer", key, from: Number(left[1]), title: left[2] ?? "", text: left[3] ?? "" };
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
// Agents name the same thing differently: a file is `file_path` to one and `path` to another.
const TELLING_ARGUMENTS = ["command", "file_path", "path", "pattern", "url", "query", "cwd"];

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

function single(event: AgentEvent, key: number, words: Words): Item | null {
    switch (event.type) {
        case "user_message":
            return messageItem(event.text, key, event.remote === true);
        case "text":
            return { kind: "text", key, text: event.text, streaming: false };
        // A reply still being written reads like a finished one, and becomes one in place.
        case "text_delta":
            return { kind: "text", key, text: event.text, streaming: true };
        case "terminal_shared":
            return { kind: "terminal", key, text: event.text };
        case "terminal_only":
            return { kind: "terminal-only", key, command: event.command };
        case "worktree_moved": {
            const { from, to, between_turns } = event;
            return { kind: "moved", key, from, to, betweenTurns: between_turns };
        }
        // A view of nothing is not drawn: there is nothing in it to see.
        case "view":
            return event.html.trim() === "" ? null : { kind: "view", key, html: event.html };
        case "forked":
            return { kind: "forked", key, from: event.from };
        case "screen": {
            const { command, title, note, sections } = event;
            return { kind: "screen", key, command, title, note, sections };
        }
        case "init":
            return sessionItem(event, key, words);
        case "turn_done":
            return {
                kind: "turn",
                key,
                isError: event.is_error,
                costUsd: event.cost_usd,
                durationMs: event.duration_ms ?? null,
            };
        case "exited":
            return exitNotice(event, key);
        default:
            return null;
    }
}

function sessionItem(
    event: Extract<AgentEvent, { type: "init" }>,
    key: number,
    words: Words,
): Item {
    const count = (status: string) =>
        event.mcp_servers.filter((server) => server.status === status).length;
    // Claude Code says which servers connected. Other agents are only known to have been given them.
    const passed = count("passed");
    const mcp =
        passed > 0
            ? `${passed} ${words.terms.mcpServers} passed to the ${words.terms.agent}`
            : `${count("connected")} of ${event.mcp_servers.length} ${words.terms.mcpServers} connected`;
    return {
        kind: "session",
        key,
        model: event.model,
        login: event.api_key_source === "none" ? "subscription" : event.api_key_source,
        mcp,
        skills: event.skills,
    };
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
    tools: Map<string, ToolItem>;
    /** The permission requests still waiting for a decision, by request id. */
    open: Map<string, Permission>;
    /** Requests that were decided or abandoned; they are left out of the result. */
    settled: Set<Item>;
    /** The proposed changes to the configuration, by request id. */
    proposals: Map<string, Extract<Item, { kind: "proposal" }>>;
    /** What commands asked the user, by request id. */
    asked: Map<string, Extract<Item, { kind: "asked" }>>;
    /** Claude Code's last usage report, so that a warning is said once. */
    limit: RateLimit | null;
    /** Where Remote Control was last shown to be on, so that it is said once until it ends. */
    remoteAt: string | null;
    /** The style's words, for what the items say of their own. */
    words: Words;
}

/** The transcript items for a session's events, in order. Tool results join their calls. */
export function toItems(events: AgentEvent[], words: Words = wordsFor("plain")): Item[] {
    const draft: Draft = {
        items: [],
        tools: new Map(),
        open: new Map(),
        settled: new Set(),
        proposals: new Map(),
        asked: new Map(),
        limit: null,
        remoteAt: null,
        words,
    };
    events.forEach((event, key) => {
        add(draft, event, key);
    });
    // A view the tool handed over stands for the call that showed it. The call itself is only
    // drawn as a view where no such handover was recorded, as in conversations from before.
    const handed = new Set(events.flatMap((event) => (event.type === "view" ? [event.html] : [])));
    const shownTwice = (item: Item) => item.kind === "tool" && handedOver(handed, item);
    return draft.items.filter((item) => !(draft.settled.has(item) || shownTwice(item)));
}

/**
 * Whether a tool call showed HTML that a view stands for. A call that shows no HTML is never
 * one: every ordinary tool shows the empty string, which an empty view must not stand for.
 */
function handedOver(handed: Set<string>, tool: ToolItem): boolean {
    const html = shownHtml(tool.name, tool.input);
    return html !== "" && handed.has(html);
}

/**
 * Whether a tool call shows whatever the setting says: one that put questions to the user,
 * presented a plan, or showed a view. The rest is the agent at work.
 */
function talksToUser(tool: ToolItem): boolean {
    const { name, input } = tool;
    return name === PLAN_TOOL || name === QUESTION_TOOL || shownHtml(name, input) !== "";
}

/**
 * The items with the agent's tool calls shown as the setting says: each on its own (`all`),
 * the ones that follow each other folded into one item (`folded`), or none (`hidden`). Calls
 * that talk to the user stay as they are, and so does everything else.
 */
export function shownTools(items: Item[], mode: ToolCalls): Item[] {
    if (mode === "all") {
        return items;
    }
    const atWork = (item: Item): item is ToolItem => item.kind === "tool" && !talksToUser(item);
    if (mode === "hidden") {
        return items.filter((item) => !atWork(item));
    }
    const shown: Item[] = [];
    for (const item of items) {
        const last = shown.at(-1);
        if (!atWork(item)) {
            shown.push(item);
        } else if (last?.kind === "tools") {
            // Made in this loop, so it is this function's own to add to.
            last.calls.push(item);
        } else {
            // Keyed by its first call, so it keeps its place in the list as calls join it.
            shown.push({ kind: "tools", key: item.key, calls: [item] });
        }
    }
    return shown;
}

/** What the line of folded tool calls says. */
export interface ToolRunSummary {
    count: number;
    /** The call made last, which the line names. */
    last: ToolItem;
    failed: number;
    /** Whether the last call has not ended yet. */
    running: boolean;
}

/** How a run of folded tool calls is summed up: how many, the last, how many failed. */
export function toolRunSummary(calls: [ToolItem, ...ToolItem[]]): ToolRunSummary {
    const last = calls.at(-1) ?? calls[0];
    return {
        count: calls.length,
        last,
        failed: calls.filter((call) => call.result?.isError === true).length,
        running: last.result === null,
    };
}

/** What of a tool call is drawn: its name, its arguments and how it ended. */
export interface ToolShown {
    name: string;
    input: unknown;
    result: ToolResult | null;
}

/**
 * Whether two tool calls are drawn alike. The transcript's items are built again for every
 * piece of a reply, with a new result object for each call, so the result is compared by what
 * it holds; the arguments are the event's own and stay the same object.
 */
export function sameToolCall(a: ToolShown, b: ToolShown): boolean {
    return (
        a.name === b.name &&
        a.input === b.input &&
        a.result?.content === b.result?.content &&
        a.result?.isError === b.result?.isError
    );
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
        case "config_proposed":
            return addProposal(draft, event, key);
        case "config_answered":
            return answerProposal(draft.proposals.get(event.request_id), event.accepted);
        case "command_asked":
            return addAsked(draft, event, key);
        case "command_answered":
            return answerAsked(draft, event, key);
        case "rate_limit":
            return addLimit(draft, event, key);
        case "remote_control":
            return addRemote(draft, event, key);
        default:
            return addSingle(draft, event, key);
    }
}

/**
 * Remote Control's line: once when it is on, with the address, however often the agent starts
 * again and attaches to the same session there; and when it is turned off or fails.
 */
function addRemote(
    draft: Draft,
    event: Extract<AgentEvent, { type: "remote_control" }>,
    key: number,
) {
    const { state, url, detail } = event;
    if (state === "on" && url !== null && url !== draft.remoteAt) {
        draft.remoteAt = url;
        draft.items.push({ kind: "remote", key, state, url, detail: null });
    } else if (state === "off" || state === "failed") {
        draft.remoteAt = null;
        draft.items.push({ kind: "remote", key, state, url: null, detail });
    }
}

function addProposal(
    draft: Draft,
    event: Extract<AgentEvent, { type: "config_proposed" }>,
    key: number,
) {
    const { request_id: requestId, title, detail } = event;
    const proposal: Item = { kind: "proposal", key, requestId, title, detail, accepted: null };
    draft.proposals.set(requestId, proposal);
    draft.items.push(proposal);
}

/** A proposal stays where it was made, and says how it was answered. */
function answerProposal(
    proposal: Extract<Item, { kind: "proposal" }> | undefined,
    accepted: boolean,
) {
    if (proposal) {
        proposal.accepted = accepted;
    }
}

function addAsked(
    draft: Draft,
    event: Extract<AgentEvent, { type: "command_asked" }>,
    key: number,
) {
    const { request_id: requestId, command } = event;
    // Questions that cannot be shown leave nothing to choose from, so nothing is drawn.
    const questions = questionsOf(QUESTION_TOOL, { questions: event.questions });
    if (questions === null) {
        return;
    }
    const asked: Item = { kind: "asked", key, requestId, command, questions, said: null };
    draft.asked.set(requestId, asked);
    draft.items.push(asked);
}

/**
 * A question says what came of it where it was asked. An answer to none that was asked is a
 * setting named with its command, as in `/model opus`: it is a line of its own.
 */
function answerAsked(
    draft: Draft,
    event: Extract<AgentEvent, { type: "command_answered" }>,
    key: number,
) {
    const asked = draft.asked.get(event.request_id);
    if (asked) {
        asked.said = event.said;
        return;
    }
    const command = event.request_id.replace(/-[^-]*$/, "");
    const { request_id: requestId, said } = event;
    draft.items.push({ kind: "asked", key, requestId, command, questions: [], said });
}

function addLimit(draft: Draft, event: RateLimit, key: number) {
    const text = limitNotice(event, draft.limit, new Date());
    draft.limit = event;
    if (text !== null) {
        draft.items.push({ kind: "notice", key, tone: "warning", text });
    }
}

function addSingle(draft: Draft, event: AgentEvent, key: number) {
    const item = single(event, key, draft.words);
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
    tool: ToolItem | undefined,
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
    if (shownHtml(running.name, running.input) !== "") {
        return "Showing a view";
    }
    return running.name === PLAN_TOOL ? "Presenting its plan" : `Running ${running.name}`;
}

/**
 * Which message of the session the event at `key` is, counting from 0 every message sent into
 * it: what a fork from that message is asked for by. The place of the event itself would not
 * do: this list also holds events that are only shown and never stored, such as the queue's,
 * so the core's list of the same session is shorter.
 */
export function messageNumber(events: AgentEvent[], key: number): number {
    return events.slice(0, key).filter((event) => event.type === "user_message").length;
}

/**
 * The latest move of the task's worktree in the conversation, by its key, or `null`. Only that
 * one can offer the way back: an earlier one's would switch away from wherever a later move,
 * or the user, has since put the worktree.
 */
export function latestMove(items: Item[]): number | null {
    return items.findLast((item) => item.kind === "moved")?.key ?? null;
}

/**
 * What the transcript says of a move of the task's worktree to another branch: what follows
 * from it when the agent moved it, and that the agent was told when it moved between turns.
 */
export function movedSaid(
    from: string | null,
    to: string | null,
    betweenTurns: boolean,
    words: Words,
): string {
    const { agent, branch, commit, pullRequest, push, task, worktree } = words.terms;
    const on = (name: string | null) => name ?? `no ${branch}`;
    if (betweenTurns) {
        return `The ${worktree} was moved from ${on(from)} to ${on(to)} since the last turn, and the ${agent} was told with this message.`;
    }
    const moved = `The ${worktree} moved from ${on(from)} to ${on(to)} during this turn`;
    if (to === null) {
        return `${moved}, so there is nothing to ${push} until it is on a ${branch} again.`;
    }
    return `${moved}: the ${task}'s changes and ${pullRequest} are ${to}'s now, and ${commit} and ${push} go to it.`;
}

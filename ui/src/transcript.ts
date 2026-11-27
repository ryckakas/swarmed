import type { AgentEvent, AppCsp, PullChange, ScreenSection, Tokens, ToolCalls } from "./api/types";
import type { ListedElement } from "./elements";
import { type GitHubMentioned, splitMentioned } from "./githubMentions";
import { shownHtml } from "./htmlview";
import { type SentContext, splitAppContexts } from "./mcpApps";
import type { Mentioned } from "./mentions";
import type { Regrouped } from "./parents";
import { QUESTION_TOOL, type Question, questionsOf } from "./questions";
import { type RateLimit, refusalNotice } from "./rateLimit";
import type { RoleChange } from "./roles";
import { type Words, wordsFor } from "./wording";

// Turns a session's raw events into what the transcript shows. Pure, so it can be tested.

export type Item =
    /**
     * What the user wrote, and the files, sessions, pull requests and issues they pointed the
     * agent at with it; `remote` for what they typed on claude.ai or in the Claude app, which
     * reached the session from there.
     */
    | {
          kind: "user";
          key: number;
          text: string;
          files: string[];
          /** The sessions listed under the message, which its tokens name. */
          sessions: Mentioned[];
          /** The pull requests and issues listed under the message, which its tokens name. */
          github: GitHubMentioned[];
          /** The elements picked in the task's browser listed under the message. */
          elements: ListedElement[];
          remote: boolean;
          /** Whether Swarmed sent it by itself, as the usage limit that stopped the session reset. */
          atReset: boolean;
          /** What apps shown in the conversation left for the agent with it. */
          contexts?: SentContext[];
      }
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
    /** Whom a session works for changed: this one, or one that works or worked for it. */
    | ({ kind: "regrouped"; key: number } & Regrouped)
    /** The session's role changed: its rules apply from here on. */
    | ({ kind: "role"; key: number } & RoleChange)
    /** Something changed on the task's pull request: checks, a review, a comment, a merge. */
    | { kind: "pull"; key: number; number: number; url: string; change: PullChange }
    /** HTML the agent showed the user through Swarmed's tool. */
    | { kind: "view"; key: number; html: string }
    | AppItem
    /**
     * The last image the agent's tools returned in a turn, again at the end of what it said
     * then, so that a run of folded calls still shows it. Keyed by the result it came in.
     */
    | { kind: "last-image"; key: number; path: string }
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
          /** The tokens the session's own agent used in it, if the agent said. */
          tokens: Tokens | null;
      }
    | { kind: "notice"; key: number; tone: "error" | "warning"; text: string }
    /**
     * A usage limit refused the turn, which stopped there: what it says, and when every limit
     * that refused it resets, in seconds since 1970, if said.
     */
    | LimitItem
    /** Where the copy of an earlier conversation ends and the fork's own begins. */
    | { kind: "forked"; key: number; from: number }
    | RewoundItem;

/**
 * Where the session was taken back to before one of its messages, with what that dropped
 * folded under it: still stored, no longer in the conversation.
 */
export interface RewoundItem {
    kind: "rewound";
    key: number;
    /** The message it was taken back to before, by its number in the session. */
    to: number;
    /** That message as sent, which names it on the line. */
    message: string;
    /** Whether the files went back too. */
    files: boolean;
    /** Whether those files were put back since. */
    putBack: boolean;
    /** Commits made since the message, which stayed on the branch, if that is known. */
    commitsSince: number | null;
    /** What it dropped, as the transcript showed it. */
    dropped: Item[];
}

/**
 * The app an MCP server attached to a call, under that call: what it is kept in, what its
 * server declared, and what the call was given and returned, which the app is told.
 */
export interface AppItem {
    kind: "app";
    key: number;
    /** The call it belongs to. */
    toolUseId: string;
    server: string;
    tool: string;
    /** The file its HTML is kept in; `null` when it was not kept, which `note` says why. */
    document: string | null;
    note: string | null;
    csp: AppCsp;
    /** The devices and the like it asked for, which it is not given. */
    permissions: string[];
    /** The tools of its own server it may call. */
    callable: string[];
    /** What the call was given. */
    input: unknown;
    /** What came of it, as an app is handed it (MCP's `CallToolResult`). */
    result: unknown;
}

/** The line a turn a usage limit refused keeps where it stopped. */
export interface LimitItem {
    kind: "limit";
    key: number;
    text: string;
    resetsAt: number | null;
}

export interface ToolResult {
    content: string;
    isError: boolean;
    /** The images it returned that were kept, as files of the session's own, in order. */
    images: string[];
}

/** One tool call, as the transcript holds it. */
export type ToolItem = {
    kind: "tool";
    key: number;
    /** The agent's id for the call. */
    id: string;
    name: string;
    input: unknown;
    result: ToolResult | null;
    /** Whether the work it waited on was moved to the background, where it goes on. */
    moved: boolean;
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

/**
 * A message the user sent, taken apart: what they wrote, the sessions listed under it, the pull
 * requests and issues after those, then the elements picked in the task's browser, and the
 * files listed last.
 */
export function splitSent(message: string): {
    text: string;
    files: string[];
    sessions: Mentioned[];
    github: GitHubMentioned[];
    elements: ListedElement[];
} {
    const { text, files } = splitFiles(message);
    return { ...splitMentioned(text), files };
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
function messageItem(
    text: string,
    key: number,
    { remote, atReset }: { remote: boolean; atReset: boolean },
): Item {
    const left = FROM_SESSION.exec(text);
    if (!left) {
        const sent = splitSent(text);
        const { text: typed, contexts } = splitAppContexts(sent.text);
        const apps = contexts.length > 0 ? { contexts } : {};
        return { kind: "user", key, ...sent, text: typed, ...apps, remote, atReset };
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
            return messageItem(event.text, key, {
                remote: event.remote === true,
                atReset: event.at_reset === true,
            });
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
        case "parent_changed": {
            const { child, parent, previous } = event;
            return { kind: "regrouped", key, child, parent, previous };
        }
        case "role_changed":
            return { kind: "role", key, role: event.role, previous: event.previous };
        case "pull_changed":
            return {
                kind: "pull",
                key,
                number: event.number,
                url: event.url,
                change: event.change,
            };
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
                tokens: event.tokens ?? null,
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
    /** The line that says a usage limit refused the turn under way, so that it is said once. */
    refused: LimitItem | null;
    /** Where Remote Control was last shown to be on, so that it is said once until it ends. */
    remoteAt: string | null;
    /** The messages sent so far, by number: where each is, and what it said. */
    messages: { key: number; text: string }[];
    /** The rewinds so far, which a putting back of files marks. */
    rewinds: RewoundItem[];
    /** The last image a call returned in the turn under way, with the key of its result. */
    lastImage: { key: number; path: string } | null;
    /** The style's words, for what the items say of their own. */
    words: Words;
}

/**
 * The transcript items for a session's events, in order. Tool results join their calls. Of
 * Claude Code's usage reports, only a turn a limit refused is said: the rest are the usage
 * notice's, at the top of the window.
 */
export function toItems(events: AgentEvent[], words: Words = wordsFor("plain")): Item[] {
    const draft: Draft = {
        items: [],
        tools: new Map(),
        open: new Map(),
        settled: new Set(),
        proposals: new Map(),
        asked: new Map(),
        refused: null,
        remoteAt: null,
        messages: [],
        rewinds: [],
        lastImage: null,
        words,
    };
    events.forEach((event, key) => {
        add(draft, event, key);
    });
    // A view the tool handed over stands for the call that showed it. The call itself is only
    // drawn as a view where no such handover was recorded, as in conversations from before.
    const handed = new Set(events.flatMap((event) => (event.type === "view" ? [event.html] : [])));
    const shownTwice = (item: Item) => item.kind === "tool" && handedOver(handed, item);
    const kept = (items: Item[]): Item[] =>
        items
            .filter((item) => !(draft.settled.has(item) || shownTwice(item)))
            .map((item) =>
                item.kind === "rewound" ? { ...item, dropped: kept(item.dropped) } : item,
            );
    return kept(draft.items);
}

/**
 * Folds what a rewind dropped under its line: the items of every event from the message it
 * took the session back to before, up to the rewind. A rewind's line inside such a part goes
 * with it, folded in turn.
 */
function addRewound(draft: Draft, event: Extract<AgentEvent, { type: "rewound" }>, key: number) {
    const message = draft.messages[event.to];
    const start = message?.key ?? key;
    const at = draft.items.findIndex((item) => item.key >= start);
    const dropped = at === -1 ? [] : draft.items.splice(at);
    const item: RewoundItem = {
        kind: "rewound",
        key,
        to: event.to,
        message: message?.text ?? "",
        files: event.files === true,
        putBack: false,
        commitsSince: event.commits_since ?? null,
        dropped,
    };
    draft.rewinds.push(item);
    draft.items.push(item);
}

/** Marks the latest rewind to before message `to` whose files went back as put back. */
function markPutBack(draft: Draft, to: number) {
    const rewind = draft.rewinds.findLast((item) => item.files && item.to === to);
    if (rewind) {
        rewind.putBack = true;
    }
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
 * The session line and the turns' end lines worth drawing, after `toItems` and before
 * `shownTools`, so that a turn of tool calls the setting hides still showed something. Claude
 * Code says what it loaded at the start of every turn: only the first is drawn, and the status
 * line says the model since. A turn's end line closes a turn that showed something; a turn
 * with nothing in it and no more spent, such as one the agent took by itself to read a report
 * that came to nothing, has none. With `turnEnds` off, only a turn that failed or was stopped
 * keeps its line, since that says what the rest does not.
 */
export function shownLines(items: Item[], turnEnds: boolean): Item[] {
    let started = false;
    let showedSince = false;
    let lastTurn: Extract<Item, { kind: "turn" }> | null = null;
    return items.filter((item) => {
        if (item.kind === "session") {
            const first = !started;
            started = true;
            return first;
        }
        if (item.kind !== "turn") {
            showedSince = true;
            return true;
        }
        const empty = !showedSince && lastTurn !== null && item.costUsd === lastTurn.costUsd;
        showedSince = false;
        lastTurn = item;
        return item.isError || (turnEnds && !empty);
    });
}

/**
 * The items with the agent's tool calls shown as the setting says: each on its own (`all`),
 * the ones that follow each other folded into one item (`folded`), or none (`hidden`). Calls
 * that talk to the user stay as they are, and so does everything else. The images calls
 * returned stay in view with a folded run; hidden, they go with the calls, the turn's last
 * one at the end of its reply too.
 */
export function shownTools(items: Item[], mode: ToolCalls): Item[] {
    if (mode === "all") {
        return items;
    }
    const atWork = (item: Item): item is ToolItem => item.kind === "tool" && !talksToUser(item);
    if (mode === "hidden") {
        return items.filter((item) => !(atWork(item) || item.kind === "last-image"));
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

/** What of a tool call is drawn: its name, its arguments, how it ended and where it went on. */
export interface ToolShown {
    id: string;
    name: string;
    input: unknown;
    result: ToolResult | null;
    moved: boolean;
}

/** The tools whose call waits on work that can go to the background: a command, a sub-agent. */
const GOES_TO_BACKGROUND = ["Bash", "Agent", "Task"];

/**
 * Whether the work a call waits on could be sent to the background now: a command or a
 * sub-agent that runs in the foreground and has not ended. One the agent started in the
 * background is there already. Whether the agent can, and is at work, is the caller's to know.
 */
export function mayGoToBackground(call: ToolShown): boolean {
    const input = (call.input ?? {}) as Record<string, unknown>;
    return (
        call.result === null &&
        GOES_TO_BACKGROUND.includes(call.name) &&
        input.run_in_background !== true
    );
}

/**
 * Whether two tool calls are drawn alike. The transcript's items are built again for every
 * piece of a reply, with a new result object for each call, so the result is compared by what
 * it holds; the arguments are the event's own and stay the same object.
 */
export function sameToolCall(a: ToolShown, b: ToolShown): boolean {
    return (
        a.id === b.id &&
        a.name === b.name &&
        a.input === b.input &&
        a.moved === b.moved &&
        a.result?.content === b.result?.content &&
        a.result?.isError === b.result?.isError &&
        sameList(a.result?.images ?? [], b.result?.images ?? [])
    );
}

function sameList(a: readonly string[], b: readonly string[]): boolean {
    return a.length === b.length && a.every((item, at) => item === b[at]);
}

function add(draft: Draft, event: AgentEvent, key: number) {
    if (CLOSES_REQUESTS.has(event.type)) {
        for (const request of draft.open.values()) {
            draft.settled.add(request);
        }
        draft.open.clear();
        draft.refused = null;
    }
    switch (event.type) {
        case "tool_use":
            return addTool(draft, event, key);
        case "tool_result":
            return attachResult(draft, event, key);
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
        case "background":
            return markMoved(draft.tools.get(event.tool_use_id ?? ""), event);
        case "rewound":
            return addRewound(draft, event, key);
        case "files_put_back":
            return markPutBack(draft, event.to);
        case "app":
            return addApp(draft, event, key);
        case "user_message":
            draft.messages.push({ key, text: event.text });
            draft.lastImage = null;
            return addSingle(draft, event, key);
        case "turn_done":
            return endTurn(draft, event, key);
        default:
            return addSingle(draft, event, key);
    }
}

/**
 * An app under the call it belongs to, which tells it what the call was given. A result not
 * kept for it is told by the call's text.
 */
function addApp(draft: Draft, event: Extract<AgentEvent, { type: "app" }>, key: number) {
    const call = draft.tools.get(event.tool_use_id);
    const text = call?.result ? { content: [{ type: "text", text: call.result.content }] } : null;
    draft.items.push({
        kind: "app",
        key,
        toolUseId: event.tool_use_id,
        server: event.server,
        tool: event.tool,
        document: event.document,
        note: event.note,
        csp: event.csp,
        permissions: event.permissions,
        callable: event.callable,
        input: call?.input ?? {},
        result: event.result ?? text,
    });
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

/**
 * A turn a usage limit refused keeps one line where it stopped, which says why. Another limit
 * that refused the same turn says nothing more, but the turn can go on only once both reset.
 */
function addLimit(draft: Draft, event: RateLimit, key: number) {
    const text = refusalNotice(event, new Date());
    if (text === null) {
        return;
    }
    if (draft.refused !== null) {
        draft.refused.resetsAt = laterOf(draft.refused.resetsAt, event.resets_at);
        return;
    }
    draft.refused = { kind: "limit", key, text, resetsAt: event.resets_at };
    draft.items.push(draft.refused);
}

function laterOf(a: number | null, b: number | null): number | null {
    return a === null || b === null ? (a ?? b) : Math.max(a, b);
}

function addSingle(draft: Draft, event: AgentEvent, key: number) {
    const item = single(event, key, draft.words);
    if (item) {
        draft.items.push(item);
    }
}

function addTool(draft: Draft, event: Extract<AgentEvent, { type: "tool_use" }>, key: number) {
    const { id, name, input } = event;
    const tool: Item = { kind: "tool", key, id, name, input, result: null, moved: false };
    draft.tools.set(id, tool);
    draft.items.push(tool);
}

/** Marks the call whose work a report says was moved to the background. */
function markMoved(tool: ToolItem | undefined, event: Extract<AgentEvent, { type: "background" }>) {
    if (tool && event.moved === true) {
        tool.moved = true;
    }
}

function attachResult(
    draft: Draft,
    event: Extract<AgentEvent, { type: "tool_result" }>,
    key: number,
) {
    const tool = draft.tools.get(event.tool_use_id);
    if (!tool) {
        return;
    }
    const images = event.images ?? [];
    tool.result = { content: event.content, isError: event.is_error, images };
    const path = images.at(-1);
    if (path !== undefined) {
        draft.lastImage = { key, path };
    }
}

/** A turn's end: its last image first, at the end of what the agent said, then its line. */
function endTurn(draft: Draft, event: Extract<AgentEvent, { type: "turn_done" }>, key: number) {
    if (draft.lastImage !== null) {
        draft.items.push({ kind: "last-image", ...draft.lastImage });
        draft.lastImage = null;
    }
    addSingle(draft, event, key);
}

/** The images a run of calls returned, in the order they came. */
export function imagesOf(calls: readonly ToolItem[]): string[] {
    return calls.flatMap((call) => call.result?.images ?? []);
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
 * The latest rewind whose files went back and were not put back, by its key, or `null`: only
 * that one offers to put them back, as only its files are kept to. One folded under a later
 * rewind is found too.
 */
export function latestFilesRewind(items: Item[]): number | null {
    const withFiles = rewindsIn(items).filter((item) => item.files);
    const latest = withFiles.reduce<RewoundItem | null>(
        (found, item) => (found === null || item.key > found.key ? item : found),
        null,
    );
    return latest !== null && !latest.putBack ? latest.key : null;
}

/** Every rewind's line among `items`, those folded under another's included. */
function rewindsIn(items: Item[]): RewoundItem[] {
    return items.flatMap((item) =>
        item.kind === "rewound" ? [item, ...rewindsIn(item.dropped)] : [],
    );
}

/**
 * The latest line that says checks of the task's pull request failed, by its key, or `null`.
 * Only that one offers to send the failure to the agent: an earlier one's checks may have run
 * again since.
 */
export function latestFailure(items: Item[]): number | null {
    const failed = (item: Item) => item.kind === "pull" && item.change.kind === "checks_failed";
    return items.findLast(failed)?.key ?? null;
}

/**
 * The latest line that says the task's pull request merged, by its key, or `null`. Only that
 * one offers to put the task away.
 */
export function latestMerge(items: Item[]): number | null {
    const merged = (item: Item) => item.kind === "pull" && item.change.kind === "merged";
    return items.findLast(merged)?.key ?? null;
}

/**
 * The line of the latest turn a usage limit refused, by its key, if nothing was written to the
 * session since: only that one can offer to go on when the limit resets. A message since, the
 * user's or another session's, went on already.
 */
export function latestLimit(items: Item[]): number | null {
    const at = items.findLastIndex((item) => item.kind === "limit");
    // A rewind since took the session back to before something written after it, or to
    // before the limit's own turn.
    const writtenSince = items
        .slice(at + 1)
        .some((item) => item.kind === "user" || item.kind === "peer" || item.kind === "rewound");
    return at === -1 || writtenSince ? null : (items[at]?.key ?? null);
}

/** What a refused turn's line offers about going on by itself when its limit resets. */
export type ContinueOffer =
    /** To go on at `at`, in seconds since 1970, if the user asks. */
    | { state: "offered"; at: number }
    /** It goes on at `at` by itself, which the user can cancel. */
    | { state: "set"; at: number }
    | null;

/**
 * What a refused turn's line offers, at `now` in seconds since 1970: on the latest such line
 * alone (`latest`), that the session goes on when it is set to (`continueAt`), or else to go on
 * when its limit resets, if that is still to come. Nothing on any other line.
 */
export function continueOffer(
    item: LimitItem,
    latest: boolean,
    continueAt: number | null,
    now: number,
): ContinueOffer {
    if (!latest) {
        return null;
    }
    if (continueAt !== null) {
        return { state: "set", at: continueAt };
    }
    const { resetsAt } = item;
    return resetsAt !== null && resetsAt > now ? { state: "offered", at: resetsAt } : null;
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

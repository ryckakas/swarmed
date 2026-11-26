// The host's side of an app an MCP server attached to a tool call (MCP Apps): the JSON-RPC
// messages the app's frame sends over `postMessage`, and what Swarmed answers or asks the user
// first. Pure, so that every decision about what an app may do is tested here; the frame and
// its listener are in `McpAppView.tsx`.
//
// Only sizing and the handshake happen by themselves. A link and a message to the conversation
// each need the user's yes, every time. A tool call goes to the session's agent, which decides
// it as its own call of the tool, and only for a tool of the app's own server that the server
// lets apps call. Anything else is refused.

/** The protocol version answered to an app that names none. */
export const PROTOCOL_VERSION = "2026-01-26";

/** JSON-RPC's code for a method this host does not have. */
export const METHOD_NOT_FOUND = -32601;
/** JSON-RPC's code for a request whose parameters cannot be used. */
export const INVALID_PARAMS = -32602;
/** The code MCP Apps uses for what the host or the user refused. */
export const REFUSED = -32000;

/** The longest context an app may leave for the agent, in characters. */
export const MAX_CONTEXT = 8000;

/** How tall an app's frame may grow: one that keeps asking for more is held there. */
export const MAX_APP_HEIGHT = 6000;

/** How tall an app's frame starts, before it says how tall it is. */
export const FIRST_APP_HEIGHT = 320;

/** A JSON-RPC request's id. */
export type RpcId = string | number;

/** A message an app sent, as far as the host reads it. */
export type Incoming =
    | { kind: "request"; id: RpcId; method: string; params: unknown }
    | { kind: "notification"; method: string; params: unknown }
    | { kind: "response"; id: RpcId };

/** Reads what an app's frame sent: a JSON-RPC 2.0 request, notification or response. */
export function readMessage(data: unknown): Incoming | null {
    if (typeof data !== "object" || data === null) {
        return null;
    }
    const message = data as Record<string, unknown>;
    if (message.jsonrpc !== "2.0") {
        return null;
    }
    const { id, method, params } = message;
    const hasId = typeof id === "string" || typeof id === "number";
    if (typeof method === "string") {
        return hasId
            ? { kind: "request", id, method, params }
            : { kind: "notification", method, params };
    }
    return hasId && ("result" in message || "error" in message) ? { kind: "response", id } : null;
}

/** What the host does about one message of an app's. */
export type Decision =
    /** Answers at once with `result`. */
    | { kind: "answer"; id: RpcId; result: unknown }
    /** Answers with an error. */
    | { kind: "refuse"; id: RpcId; code: number; message: string }
    /** Answers the handshake with the host's context. */
    | { kind: "initialize"; id: RpcId; version: string }
    /** The app is ready: what it was called with and what came of it are sent. */
    | { kind: "initialized" }
    /** Sizes the frame. */
    | { kind: "size"; height: number }
    /** Asks the user whether to open `url`. */
    | { kind: "link"; id: RpcId; url: string }
    /** Asks the user whether to send `text` to the conversation. */
    | { kind: "message"; id: RpcId; text: string }
    /** Keeps `text` for the agent with the user's next message; `null` takes it away. */
    | { kind: "context"; id: RpcId; text: string | null }
    /**
     * Calls a tool of the app's own server through the agent. `progress` is the token the app
     * gave to be told the call still runs, which keeps it waiting while the user decides.
     */
    | {
          kind: "call";
          id: RpcId;
          name: string;
          arguments: Record<string, unknown>;
          progress: RpcId | null;
      }
    /** Nothing to do. */
    | { kind: "ignore" };

/** What `params` holds, as a record; nothing if it is not one. */
function record(params: unknown): Record<string, unknown> {
    return typeof params === "object" && params !== null && !Array.isArray(params)
        ? (params as Record<string, unknown>)
        : {};
}

/**
 * What the host does about `incoming`, from an app whose server lets it call the tools in
 * `callable`. A request is always answered, a notification never.
 */
export function decide(incoming: Incoming, callable: readonly string[]): Decision {
    if (incoming.kind === "response") {
        return { kind: "ignore" };
    }
    if (incoming.kind === "notification") {
        return noticed(incoming.method, record(incoming.params));
    }
    return requested(incoming.id, incoming.method, record(incoming.params), callable);
}

function noticed(method: string, params: Record<string, unknown>): Decision {
    if (method === "ui/notifications/initialized") {
        return { kind: "initialized" };
    }
    if (method === "ui/notifications/size-changed") {
        const { height } = params;
        const usable = typeof height === "number" && Number.isFinite(height) && height > 0;
        return usable ? { kind: "size", height: Math.ceil(height) } : { kind: "ignore" };
    }
    // Logging, a request to be taken down and anything newer are not acted on.
    return { kind: "ignore" };
}

function requested(
    id: RpcId,
    method: string,
    params: Record<string, unknown>,
    callable: readonly string[],
): Decision {
    const refuse = (code: number, message: string): Decision => ({
        kind: "refuse",
        id,
        code,
        message,
    });
    switch (method) {
        case "ui/initialize": {
            const { protocolVersion } = params;
            const version = typeof protocolVersion === "string" ? protocolVersion : "";
            return { kind: "initialize", id, version: version || PROTOCOL_VERSION };
        }
        case "ping":
            return { kind: "answer", id, result: {} };
        case "ui/request-display-mode":
            return { kind: "answer", id, result: { mode: "inline" } };
        case "ui/open-link": {
            const url = linkOf(params.url);
            return url === null
                ? refuse(INVALID_PARAMS, "Only web and mail addresses are opened.")
                : { kind: "link", id, url };
        }
        case "ui/message":
            return appMessage(id, textOf(params.content));
        case "ui/update-model-context":
            return { kind: "context", id, text: contextOf(params) };
        case "tools/call":
            return toolCall(id, params, callable);
        default:
            return refuse(METHOD_NOT_FOUND, `Swarmed does not offer ${method} to apps.`);
    }
}

/**
 * A `/` with nothing before it that reads as text. The core takes a command's name after
 * trimming as Rust does, which drops more than `trim` here (U+0085 for one), so whatever
 * could be trimmed or not be seen counts as nothing.
 */
const COMMAND_START = /^[\s\p{White_Space}\p{Cc}\p{Cf}]*\//u;

/**
 * A message an app asks to send, if it is one: words, never a command or a skill, which a
 * leading `/` would make it.
 */
function appMessage(id: RpcId, text: string): Decision {
    if (text === "") {
        return { kind: "refuse", id, code: INVALID_PARAMS, message: "The message has no text." };
    }
    if (COMMAND_START.test(text)) {
        const message = "An app sends messages, not commands.";
        return { kind: "refuse", id, code: REFUSED, message };
    }
    return { kind: "message", id, text };
}

/** A tool call an app asked for, if it may: a tool of its own server that its server lets apps call. */
function toolCall(
    id: RpcId,
    params: Record<string, unknown>,
    callable: readonly string[],
): Decision {
    const { name } = params;
    if (typeof name !== "string" || name === "") {
        return { kind: "refuse", id, code: INVALID_PARAMS, message: "The call names no tool." };
    }
    if (!callable.includes(name)) {
        return {
            kind: "refuse",
            id,
            code: REFUSED,
            message: `This app may not call ${name}.`,
        };
    }
    const { progressToken } = record(params._meta);
    const progress =
        typeof progressToken === "string" || typeof progressToken === "number"
            ? progressToken
            : null;
    return { kind: "call", id, name, arguments: record(params.arguments), progress };
}

/** What tells an app that a call it waits on still runs, the `count`th time. */
export function stillRunning(token: RpcId, count: number) {
    return notification("notifications/progress", {
        progressToken: token,
        progress: count,
        message: "Waiting for the agent, which may ask the user first.",
    });
}

/** The address an app asks to open, if it is a web or mail address: anything else is not opened. */
export function linkOf(url: unknown): string | null {
    return typeof url === "string" && /^(?:https?:\/\/|mailto:)\S/i.test(url.trim())
        ? url.trim()
        : null;
}

/** The text of MCP content: one block or a list of them, text blocks only, joined by lines. */
export function textOf(content: unknown): string {
    const blocks = Array.isArray(content) ? content : [content];
    return blocks
        .map((block) => record(block))
        .filter((block) => block.type === "text" && typeof block.text === "string")
        .map((block) => block.text as string)
        .join("\n")
        .trim();
}

/**
 * The context an app leaves for the agent: its text, then its structured content as JSON, cut
 * at `MAX_CONTEXT`; `null` for none, which takes the app's context away.
 */
export function contextOf(params: Record<string, unknown>): string | null {
    const text = textOf(params.content);
    const structured =
        params.structuredContent === undefined ? "" : JSON.stringify(params.structuredContent);
    const both = [text, structured].filter((part) => part !== "").join("\n");
    if (both === "") {
        return null;
    }
    return both.length > MAX_CONTEXT ? `${both.slice(0, MAX_CONTEXT)}…` : both;
}

/** A JSON-RPC answer. */
export function answer(id: RpcId, result: unknown) {
    return { jsonrpc: "2.0", id, result };
}

/** A JSON-RPC error answer. */
export function failure(id: RpcId, code: number, message: string) {
    return { jsonrpc: "2.0", id, error: { code, message } };
}

/** A JSON-RPC notification. */
export function notification(method: string, params: unknown) {
    return { jsonrpc: "2.0", method, params };
}

/** The request that tells an app it is about to be taken away. */
export function teardown(id: RpcId) {
    return {
        jsonrpc: "2.0",
        id,
        method: "ui/resource-teardown",
        params: { reason: "The conversation no longer shows the app." },
    };
}

/**
 * What an app is told once it is ready: what its call was given, then what came of it. A call
 * with no result kept is told by its text alone.
 */
export function startNotifications(input: unknown, result: unknown) {
    return [
        notification("ui/notifications/tool-input", { arguments: record(input) }),
        notification("ui/notifications/tool-result", result ?? { content: [] }),
    ];
}

/** The app's colours and type, taken from the window's theme. */
export interface AppTheme {
    /** The palette's tokens by name, without their dashes: `panel`, `text`. */
    tokens: Record<string, string>;
    fontFamily: string;
    monoFamily: string;
    fontSize: string;
    dark: boolean;
}

/**
 * MCP Apps' style variables from the window's palette. Every one is given, so that an app has
 * nothing of its own defaults to mix with the palette's.
 */
export function styleVariables(theme: AppTheme): Record<string, string> {
    const t = (name: string, fallback: string) => theme.tokens[name] || fallback;
    const [panel, raised, dusk] = [
        t("panel", "#ffffff"),
        t("raised", "#ffffff"),
        t("dusk", "#eeeeee"),
    ];
    const [text, muted, faint, line] = [
        t("text", "#111111"),
        t("muted", "#555555"),
        t("faint", "#888888"),
        t("line", "#dddddd"),
    ];
    const [sky, clay, moss, pollen] = [
        t("sky", "#1f5fd1"),
        t("clay", "#b03c2a"),
        t("moss", "#2c7443"),
        t("pollen", "#8a5d00"),
    ];
    const statuses = { info: sky, danger: clay, success: moss, warning: pollen };
    const variables: Record<string, string> = {
        "--color-background-primary": panel,
        "--color-background-secondary": raised,
        "--color-background-tertiary": dusk,
        "--color-background-inverse": text,
        "--color-background-ghost": "transparent",
        "--color-background-disabled": dusk,
        "--color-text-primary": text,
        "--color-text-secondary": muted,
        "--color-text-tertiary": faint,
        "--color-text-inverse": panel,
        "--color-text-ghost": faint,
        "--color-text-disabled": faint,
        "--color-border-primary": line,
        "--color-border-secondary": line,
        "--color-border-tertiary": line,
        "--color-border-inverse": text,
        "--color-border-ghost": "transparent",
        "--color-border-disabled": line,
        "--color-ring-primary": sky,
        "--color-ring-secondary": muted,
        "--color-ring-inverse": panel,
        "--font-sans": theme.fontFamily,
        "--font-mono": theme.monoFamily,
        "--font-weight-normal": "400",
        "--font-weight-medium": "500",
        "--font-weight-semibold": "600",
        "--font-weight-bold": "700",
        "--border-radius-xs": "2px",
        "--border-radius-sm": "4px",
        "--border-radius-md": "6px",
        "--border-radius-lg": "8px",
        "--border-radius-xl": "12px",
        "--border-radius-full": "9999px",
        "--border-width-regular": "1px",
        "--shadow-hairline": `0 0 0 1px ${line}`,
        "--shadow-sm": "0 1px 2px rgba(0, 0, 0, 0.12)",
        "--shadow-md": "0 2px 8px rgba(0, 0, 0, 0.16)",
        "--shadow-lg": "0 8px 24px rgba(0, 0, 0, 0.2)",
    };
    for (const [status, colour] of Object.entries(statuses)) {
        variables[`--color-background-${status}`] = `color-mix(in srgb, ${colour} 14%, ${panel})`;
        variables[`--color-text-${status}`] = colour;
        variables[`--color-border-${status}`] = colour;
        variables[`--color-ring-${status}`] = colour;
    }
    const base = Number.parseFloat(theme.fontSize) || 13;
    const sizes = { xs: 0.85, sm: 0.92, md: 1, lg: 1.15 };
    for (const [name, scale] of Object.entries(sizes)) {
        variables[`--font-text-${name}-size`] = `${Math.round(base * scale)}px`;
        variables[`--font-text-${name}-line-height`] = "1.45";
    }
    const headings = { xs: 1, sm: 1.1, md: 1.25, lg: 1.45, xl: 1.7, "2xl": 2, "3xl": 2.4 };
    for (const [name, scale] of Object.entries(headings)) {
        variables[`--font-heading-${name}-size`] = `${Math.round(base * scale)}px`;
        variables[`--font-heading-${name}-line-height`] = "1.25";
    }
    return variables;
}

/** Where the app is shown, as MCP Apps' host context says it. */
export interface Placement {
    toolUseId: string;
    tool: string;
    /** How wide the frame is, in pixels. */
    width: number;
    locale: string;
    timeZone: string;
}

/** The host context an app is told: theme, type, size and where it is. */
export function hostContext(theme: AppTheme, placement: Placement) {
    return {
        toolInfo: {
            id: placement.toolUseId,
            tool: { name: placement.tool, inputSchema: { type: "object" } },
        },
        theme: theme.dark ? "dark" : "light",
        styles: { variables: styleVariables(theme) },
        displayMode: "inline",
        availableDisplayModes: ["inline"],
        containerDimensions: { width: Math.round(placement.width), maxHeight: MAX_APP_HEIGHT },
        locale: placement.locale,
        timeZone: placement.timeZone,
        userAgent: "Swarmed",
        platform: "desktop",
        deviceCapabilities: { touch: false, hover: true },
    };
}

/**
 * What the host can do for an app, as its answer to the handshake says: tools only for an app
 * whose server lets it call some.
 */
export function hostCapabilities(calls: boolean) {
    return {
        openLinks: {},
        ...(calls ? { serverTools: {} } : {}),
        logging: {},
        message: { text: {} },
        updateModelContext: { text: {}, structuredContent: {} },
        // No device is granted, whatever the app asked for.
        sandbox: { permissions: {} },
    };
}

/** The answer to an app's handshake. */
export function initializeResult(
    version: string,
    theme: AppTheme,
    placement: Placement,
    capabilities: ReturnType<typeof hostCapabilities>,
) {
    return {
        protocolVersion: version,
        hostInfo: { name: "Swarmed", version: "1" },
        hostCapabilities: capabilities,
        hostContext: hostContext(theme, placement),
    };
}

/**
 * The key a yes to load an app that reaches the network is remembered by: its server and what
 * it reaches. The same server's apps that reach the same places are shown without asking
 * again until Swarmed quits; one that reaches anywhere else asks.
 */
export function consentKey(server: string, reaches: readonly string[]): string {
    return JSON.stringify([server, [...reaches].sort()]);
}

/** A context an app left for the agent, to go with the user's next message. */
export interface AppContext {
    /** The call whose app left it: an app's latest takes the place of its earlier one. */
    toolUseId: string;
    server: string;
    text: string;
}

/** How a context is sent: in a tag that names its server, which the transcript finds again. */
const CONTEXT_BLOCK = /\n*<app_context server="([^"\n]*)">\n([\s\S]*?)\n<\/app_context>/g;

/** A server's name as it can stand in the tag. */
function inTag(server: string): string {
    return server.replace(/["\n<>]/g, "'");
}

/**
 * What an app left, as it can stand inside the tag: a tag of the same name in it, opening or
 * closing, is renamed, so that it can neither end its block early, and pass what follows off
 * as the user's words, nor open one that names another server.
 */
function inBlock(text: string): string {
    return text.replace(/<(\/?)\s*app_context/gi, "<$1app-context");
}

/** `message` with the contexts apps left after it, each in its tag. */
export function withAppContexts(
    message: string,
    contexts: readonly Pick<AppContext, "server" | "text">[],
): string {
    const blocks = contexts.map(
        (context) =>
            `<app_context server="${inTag(context.server)}">\n${inBlock(context.text)}\n</app_context>`,
    );
    return [message, ...blocks].filter((part) => part !== "").join("\n\n");
}

/** A context as the transcript shows it under a message. */
export interface SentContext {
    server: string;
    text: string;
}

/** A sent message taken apart: what was typed, and the contexts apps left with it. */
export function splitAppContexts(message: string): { text: string; contexts: SentContext[] } {
    const contexts: SentContext[] = [];
    const text = message.replace(CONTEXT_BLOCK, (_block, server: string, said: string) => {
        contexts.push({ server, text: said });
        return "";
    });
    return contexts.length === 0 ? { text: message, contexts } : { text, contexts };
}

/** What apps left for the agent, by session. */
export type AppContexts = Record<number, AppContext[]>;

/** `all` with session `sessionId`'s contexts set to `contexts`, or dropped when there are none. */
function withSession(all: AppContexts, sessionId: number, contexts: AppContext[]): AppContexts {
    const rest = Object.fromEntries(
        Object.entries(all).filter(([id]) => Number(id) !== sessionId),
    ) as AppContexts;
    return contexts.length === 0 ? rest : { ...rest, [sessionId]: contexts };
}

/**
 * `all` with what the app of call `toolUseId` left for session `sessionId` in place of its
 * earlier context, or without it for `null`: each app's latest takes the place of the one
 * before, and goes last.
 */
export function contextsWith(
    all: AppContexts,
    sessionId: number,
    toolUseId: string,
    context: AppContext | null,
): AppContexts {
    const others = (all[sessionId] ?? []).filter((one) => one.toolUseId !== toolUseId);
    return withSession(all, sessionId, context === null ? others : [...others, context]);
}

/** What apps left for session `sessionId`, taken to go with a message, and what is left of `all`. */
export function contextsTaken(all: AppContexts, sessionId: number) {
    return { taken: all[sessionId] ?? [], rest: withSession(all, sessionId, []) };
}

/**
 * `all` with `contexts` put back for session `sessionId`, when the message they went with was
 * not sent. What an app left since then is newer, and stays in place of its earlier one.
 */
export function contextsBack(
    all: AppContexts,
    sessionId: number,
    contexts: readonly AppContext[],
): AppContexts {
    const since = all[sessionId] ?? [];
    const newer = (one: AppContext) => since.some((now) => now.toolUseId === one.toolUseId);
    return withSession(all, sessionId, [...contexts.filter((one) => !newer(one)), ...since]);
}

/** `allowed` with `key` among them, once. */
export function withAllowed(allowed: readonly string[], key: string): string[] {
    return allowed.includes(key) ? [...allowed] : [...allowed, key];
}

// What the sample agent's `app` message shows: a call of an MCP tool whose server attached an
// app to it (MCP Apps). The app draws the call's bars in the host's colours, and its buttons
// call its server's `refresh` tool, ask to send a message, leave a context for the agent and
// ask to open a link, as a real app does over `postMessage`. Its server declares one address
// to connect to, so the app asks before it loads, and one it may not have, which is named.

import type { AgentEvent, AppCsp, AppShown } from "./types";

/** The app, written by hand in place of an SDK: JSON-RPC over `postMessage` to the host. */
export const MOCK_APP_HTML = `<!doctype html>
<html><head><meta charset="utf-8"><style>
:root { color-scheme: light dark; }
body { margin: 0; padding: 14px 16px; font-family: var(--font-sans, system-ui);
    color: var(--color-text-primary, #222); background: var(--color-background-primary, #fff); }
h3 { margin: 0 0 4px; font-size: 1em; }
p { margin: 0 0 12px; color: var(--color-text-secondary, #666); font-size: 0.9em; }
.bars { display: flex; align-items: flex-end; gap: 10px; height: 140px;
    border-bottom: 1px solid var(--color-border-primary, #ddd); }
.bar { flex: 1; background: var(--color-ring-primary, #36c); border-radius: 4px 4px 0 0;
    transition: height 0.3s ease; }
.row { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 12px; }
button { font: inherit; font-size: 0.85em; padding: 3px 10px; border-radius: 6px; cursor: pointer;
    border: 1px solid var(--color-border-primary, #ccc); background: none; color: inherit; }
#said { min-height: 1.2em; margin-top: 8px; font-size: 0.85em;
    color: var(--color-text-secondary, #666); }
</style></head><body>
<h3>Invoices by month</h3>
<p id="input">Waiting for the call…</p>
<div class="bars" id="bars"></div>
<div class="row">
  <button id="refresh">Refresh</button>
  <button id="ask">Ask the agent about it</button>
  <button id="keep">Keep the tallest for the agent</button>
  <button id="docs">Open the docs</button>
</div>
<div id="said"></div>
<script>
let next = 1;
const waiting = new Map();
let bars = [];
const send = (message) => window.parent.postMessage(message, "*");
const request = (method, params) => new Promise((resolve, reject) => {
    const id = next++;
    waiting.set(id, { resolve, reject });
    send({ jsonrpc: "2.0", id, method, params });
});
const say = (text) => { document.getElementById("said").textContent = text; };
const draw = (values) => {
    bars = values;
    const top = Math.max(1, ...values);
    document.getElementById("bars").innerHTML = values
        .map((v) => '<div class="bar" style="height:' + (100 * v / top) + '%"></div>').join("");
};
window.addEventListener("message", (event) => {
    const message = event.data || {};
    if (message.id !== undefined && waiting.has(message.id)) {
        const { resolve, reject } = waiting.get(message.id);
        waiting.delete(message.id);
        return message.error ? reject(new Error(message.error.message)) : resolve(message.result);
    }
    if (message.method === "ui/notifications/tool-input") {
        document.getElementById("input").textContent =
            "Called with " + JSON.stringify(message.params.arguments);
    }
    if (message.method === "ui/notifications/tool-result") {
        draw((message.params.structuredContent || {}).bars || []);
    }
    if (message.method === "ui/resource-teardown") {
        send({ jsonrpc: "2.0", id: message.id, result: {} });
    }
});
const page = document.documentElement;
new ResizeObserver(() => send({ jsonrpc: "2.0", method: "ui/notifications/size-changed",
    params: { width: page.scrollWidth, height: Math.ceil(page.getBoundingClientRect().height) } }))
    .observe(page);
request("ui/initialize", { protocolVersion: "2026-01-26", appInfo: { name: "chart", version: "1" },
        appCapabilities: {} })
    .then((result) => {
        const variables = (result.hostContext.styles || {}).variables || {};
        for (const [name, value] of Object.entries(variables)) {
            document.documentElement.style.setProperty(name, value);
        }
        send({ jsonrpc: "2.0", method: "ui/notifications/initialized", params: {} });
    });
document.getElementById("refresh").onclick = () =>
    request("tools/call", { name: "refresh", arguments: { months: 6 } })
        .then((result) => { draw(result.structuredContent.bars); say("Refreshed."); })
        .catch((error) => say(error.message));
document.getElementById("ask").onclick = () =>
    request("ui/message", { role: "user",
        content: [{ type: "text", text: "Why is month " + (bars.indexOf(Math.max(...bars)) + 1) +
            " the highest in the invoice chart?" }] })
        .then(() => say("Sent."), (error) => say(error.message));
document.getElementById("keep").onclick = () =>
    request("ui/update-model-context", { content: [{ type: "text",
        text: "In the invoice chart the user looked at, the tallest bar is " + Math.max(...bars) + "." }] })
        .then(() => say("Kept for the agent."));
document.getElementById("docs").onclick = () =>
    request("ui/open-link", { url: "https://modelcontextprotocol.io/extensions/apps" })
        .then(() => say("Opened."), (error) => say(error.message));
</script></body></html>`;

/** What the sample server declares its app may reach. */
const MOCK_CSP: AppCsp = {
    connect: ["https://api.invoices.example.com", "http://plain.example.com"],
    resources: [],
    frames: [],
    base_uris: [],
};

/** The call of the sample tool, its result and its app, as the core records them. */
export function mockAppCall(call: string, folder: string): AgentEvent[] {
    const input = { months: 6 };
    const result = {
        content: [{ type: "text", text: "Drew the invoices of the last 6 months." }],
        structuredContent: { bars: [12, 18, 9, 21, 17, 24] },
    };
    return [
        { type: "tool_use", id: call, name: "mcp__invoices__chart", input },
        {
            type: "tool_result",
            tool_use_id: call,
            content: "Drew the invoices of the last 6 months.",
            is_error: false,
        },
        {
            type: "app",
            tool_use_id: call,
            server: "invoices",
            tool: "chart",
            uri: "ui://invoices/chart",
            document: `${folder}/app-0123456789abcdef.html`,
            note: null,
            csp: MOCK_CSP,
            permissions: ["camera"],
            prefers_border: true,
            callable: ["chart", "refresh"],
            result,
        },
    ];
}

/** What the core's policy makes of a declaration, near enough for the sample. */
export function mockAppShown(csp: AppCsp): AppShown & { address: string } {
    const declared = [...csp.connect, ...csp.resources];
    const usable = (origin: string) => /^(?:https|wss):\/\/[a-z0-9.*-]+\.[a-z]+$/.test(origin);
    return {
        id: "mock",
        policy: "default-src 'none'",
        reaches: declared.filter(usable),
        refused: [
            ...declared
                .filter((origin) => !usable(origin))
                .map((o) => `${o} (not an https origin)`),
            ...csp.frames.map((frame) => `${frame} (frames are not shown)`),
        ],
        // A browser has no custom scheme; a blob address is enough to see the app.
        address: URL.createObjectURL(new Blob([MOCK_APP_HTML], { type: "text/html" })),
    };
}

/** What the sample server's tools answer an app: new bars for `refresh`, the same for `chart`. */
export async function mockAppCalled(tool: string): Promise<unknown> {
    if (tool !== "refresh" && tool !== "chart") {
        throw new Error(`this app may not call the tool \`${tool}\``);
    }
    await new Promise((resolve) => setTimeout(resolve, 400));
    const bars = Array.from({ length: 6 }, () => 5 + Math.round(Math.random() * 20));
    return { content: [{ type: "text", text: "Drew new bars." }], structuredContent: { bars } };
}

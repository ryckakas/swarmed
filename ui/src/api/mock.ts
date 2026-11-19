import type { Api } from "./client";
import type {
    AgentEvent,
    ApiKeyInfo,
    FileChange,
    FileDiff,
    HookOutput,
    Project,
    Session,
    SessionEvent,
    SessionSettings,
    SessionStatus,
    Settings,
    Task,
    TerminalOutput,
} from "./types";

// Sample data and a scripted agent, for working on the UI in a browser where there is no Tauri
// shell. The agent behaves like the core's FakeBackend: plain text is echoed, `run: <command>`
// asks permission first, and `slow` runs until interrupted.

function session(id: number, taskId: number, title: string, status: SessionStatus): Session {
    return {
        id,
        task_id: taskId,
        agent: "claude",
        title,
        agent_session_id: `mock-${id}`,
        model: null,
        supervision: "ask",
        status,
    };
}

function task(id: number, projectId: number, name: string, sessions: Session[]): Task {
    return {
        id,
        project_id: projectId,
        name,
        branch: `ricardas/${name}`,
        base_ref: "origin/main",
        worktree_path: `/Users/you/swarmed/hero/${name}`,
        slot: id,
        setup: "ok",
        archived: false,
        sessions,
        running: sessions.filter((s) => s.status !== "idle").map((s) => s.id),
        url: `http://localhost:${3000 + id * 100}`,
    };
}

export function sampleProjects(): Project[] {
    return [
        {
            id: 1,
            name: "hero",
            repo_path: "/Users/you/code/hero",
            tasks: [
                task(1, 1, "fix-login-redirect", [
                    session(1, 1, "Fix the redirect loop after login", "waiting"),
                    session(2, 1, "Write a regression test for the loop", "working"),
                ]),
                task(2, 1, "invoice-export", [
                    session(3, 2, "Why does the CSV export drop the VAT column?", "idle"),
                ]),
                task(3, 1, "pm-16037", []),
            ],
        },
        {
            id: 2,
            name: "bonsai-lint",
            repo_path: "/Users/you/code/bonsai-lint",
            tasks: [
                task(4, 2, "rust-support", [
                    session(4, 4, "Sketch a Rust language spec", "errored"),
                    session(5, 4, "A very long conversation", "idle"),
                ]),
            ],
        },
        { id: 3, name: "fsum", repo_path: "/Users/you/code/fsum", tasks: [] },
    ];
}

const INIT: AgentEvent = {
    type: "init",
    agent_session_id: "mock",
    model: "claude-opus-5-5",
    permission_mode: "acceptEdits",
    api_key_source: "none",
    mcp_servers: [
        { name: "context7", status: "connected" },
        { name: "hero-mcp", status: "failed" },
    ],
    skills: 89,
    slash_commands: [],
};

const REDIRECT_FIX = `I found the cause. \`redirectAfterLogin\` trusts the \`next\` parameter, and the login page sets \`next\` to itself when the session cookie is missing.

Two changes:

1. Reject a \`next\` that points back at \`/login\`.
2. Fall back to the dashboard when \`next\` is not a same-origin path.

\`\`\`ts
export function safeNext(next: string | null): string {
    if (!next || !next.startsWith("/") || next.startsWith("/login")) {
        return "/dashboard";
    }
    return next;
}
\`\`\`

The existing tests pass. I'd like to push the branch so CI runs the browser tests too.`;

/** What the sample sessions have said so far. */
export function sampleEvents(sessionId: number): AgentEvent[] {
    if (sessionId === 1) {
        return [
            {
                type: "user_message",
                text: "Users get stuck in a redirect loop after login. Fix it.",
            },
            INIT,
            { type: "text", text: "I'll look at how the login redirect is built." },
            { type: "tool_use", id: "t1", name: "Grep", input: { pattern: "redirectAfterLogin" } },
            {
                type: "tool_result",
                tool_use_id: "t1",
                content: "src/auth/login.ts:41",
                is_error: false,
            },
            { type: "tool_use", id: "t2", name: "Read", input: { file_path: "src/auth/login.ts" } },
            {
                type: "tool_result",
                tool_use_id: "t2",
                content: "export function…",
                is_error: false,
            },
            { type: "tool_use", id: "t3", name: "Edit", input: { file_path: "src/auth/login.ts" } },
            { type: "tool_result", tool_use_id: "t3", content: "Applied 1 edit", is_error: false },
            { type: "tool_use", id: "t4", name: "Bash", input: { command: "npm test -- auth" } },
            { type: "tool_result", tool_use_id: "t4", content: "42 passed", is_error: false },
            { type: "text", text: REDIRECT_FIX },
            {
                type: "tool_use",
                id: "t5",
                name: "Bash",
                input: { command: "git push -u origin HEAD" },
            },
            {
                type: "permission_request",
                request_id: "sample-request",
                tool_name: "Bash",
                input: { command: "git push -u origin HEAD" },
                description: "Push the task branch to origin",
            },
        ];
    }
    if (sessionId === 4) {
        return [
            { type: "user_message", text: "Sketch a Rust language spec for bonsai-lint." },
            INIT,
            { type: "tool_use", id: "t1", name: "Bash", input: { command: "cargo metadata" } },
            {
                type: "tool_result",
                tool_use_id: "t1",
                content: "error: no Cargo.toml",
                is_error: true,
            },
            { type: "exited", code: 1, stderr: "API Error: 529 overloaded" },
        ];
    }
    if (sessionId === 5) {
        return Array.from({ length: 2000 }, (_, index): AgentEvent => {
            const text = `Message ${index + 1}. Some **markdown**, a \`code span\` and a second sentence so the row has a realistic height.`;
            return index % 2 === 0 ? { type: "user_message", text } : { type: "text", text };
        });
    }
    return [];
}

type Listener = (event: SessionEvent) => void;

/** How long the mock agent takes between events, in milliseconds. */
const PACE = 350;

const SAMPLE_CHANGES: FileChange[] = [
    { path: "public/logo.png", kind: "modified", added: null, removed: null },
    { path: "src/auth/login.ts", kind: "modified", added: 9, removed: 2 },
    { path: "src/auth/safe-next.ts", kind: "added", added: 6, removed: 0 },
    { path: "test/auth/old-redirect.test.ts", kind: "deleted", added: 0, removed: 41 },
];

const SAMPLE_BINARY_DIFF =
    "diff --git a/public/logo.png b/public/logo.png\nBinary files a/public/logo.png and b/public/logo.png differ\n";

const SAMPLE_DIFF = `diff --git a/src/auth/login.ts b/src/auth/login.ts
index 1a2b3c4..5d6e7f8 100644
--- a/src/auth/login.ts
+++ b/src/auth/login.ts
@@ -12,9 +12,16 @@ export async function login(request: Request) {
     const session = await readSession(request);
     const next = new URL(request.url).searchParams.get("next");
-    if (!session) {
-        return redirect(\`/login?next=\${next}\`);
+    if (!session && !isLoginPage(request)) {
+        return redirect(\`/login?next=\${encodeURIComponent(safeNext(next))}\`);
     }
     return redirectAfterLogin(next);
 }
+
+/** Where to go after logging in: never back to the login page itself. */
+export function safeNext(next: string | null): string {
+    if (!next || !next.startsWith("/") || next.startsWith("/login")) {
+        return "/dashboard";
+    }
+    return next;
+}
`;

async function sampleChanges(_project: string, name: string): Promise<FileChange[]> {
    return name === "pm-16037" ? [] : SAMPLE_CHANGES;
}

async function sampleDiff(_project: string, _name: string, path: string): Promise<FileDiff> {
    return { text: path.endsWith(".png") ? SAMPLE_BINARY_DIFF : SAMPLE_DIFF, truncated: false };
}

/** What a status line script would print: here, just its own name and the session. */
function scriptedStatus(command: string, id: number): string | null {
    return command === "" ? null : `${command} (session ${id})`;
}

function storeKey(keys: ApiKeyInfo[], name: string, stored: boolean) {
    const key = keys.find((candidate) => candidate.name === name);
    if (!key) {
        throw new Error(`\`${name}\` is not an API key Swarmed can store`);
    }
    key.stored = stored;
}

const PROMPT = "\u001b[32m~/swarmed/task\u001b[0m $ ";

/** A pretend shell: it echoes what is typed and answers every command the same way. */
class MockTerminal {
    readonly listeners = new Set<(output: TerminalOutput) => void>();
    private readonly printed = new Map<number, string>();

    open(id: number): string {
        const replay = this.printed.get(id);
        if (replay === undefined) {
            // Marked as started at once, so opening it twice in a row starts one shell.
            this.printed.set(id, "");
            setTimeout(() => this.print(id, PROMPT), 0);
        }
        return replay ?? "";
    }

    type(id: number, data: string) {
        const entered = data.includes("\r");
        this.print(id, entered ? `\r\n(the sample shell runs nothing)\r\n${PROMPT}` : data);
    }

    private print(id: number, data: string) {
        this.printed.set(id, (this.printed.get(id) ?? "") + data);
        for (const listener of this.listeners) {
            listener({ session_id: id, data });
        }
    }
}

const SAMPLE_CONTEXT: AgentEvent = {
    type: "context_usage",
    used: 29_629,
    max: 1_000_000,
    categories: [
        { name: "System prompt", tokens: 3338, kind: "used" },
        { name: "System tools", tokens: 3317, kind: "used" },
        { name: "MCP tools (deferred)", tokens: 129_532, kind: "deferred" },
        { name: "Skills", tokens: 9904, kind: "used" },
        { name: "Messages", tokens: 9171, kind: "used" },
        { name: "Autocompact buffer", tokens: 33_000, kind: "buffer" },
        { name: "Free space", tokens: 937_371, kind: "free" },
    ],
};

class MockAgent {
    readonly projects = sampleProjects();
    readonly events = new Map<number, AgentEvent[]>();
    readonly listeners = new Set<Listener>();
    /** What to do when a session's pending permission request is answered. */
    private readonly decisions = new Map<number, (allowed: boolean) => void>();
    private readonly interrupts = new Map<number, () => void>();
    private nextId = 100;

    history(id: number): AgentEvent[] {
        let events = this.events.get(id);
        if (!events) {
            events = sampleEvents(id);
            this.events.set(id, events);
        }
        return events;
    }

    publish(sessionId: number, event: AgentEvent, status: SessionStatus | null) {
        this.history(sessionId).push(event);
        for (const listener of this.listeners) {
            listener({ session_id: sessionId, status, event });
        }
    }

    /** Publishes a piece of a reply without keeping it, as the core does. */
    stream(sessionId: number, event: AgentEvent, steps: number) {
        setTimeout(() => {
            for (const listener of this.listeners) {
                listener({ session_id: sessionId, status: null, event });
            }
        }, PACE * steps);
    }

    later(sessionId: number, event: AgentEvent, status: SessionStatus | null, steps = 1) {
        setTimeout(() => this.publish(sessionId, event, status), PACE * steps);
    }

    newSession(taskId: number): Session {
        const created = session(this.nextId++, taskId, "", "idle");
        this.events.set(created.id, []);
        return created;
    }

    send(id: number, text: string) {
        this.publish(id, { type: "user_message", text }, "working");
        const done: AgentEvent = {
            type: "turn_done",
            is_error: false,
            result: null,
            cost_usd: 0.0123,
        };
        if (text === "slow") {
            this.later(id, { type: "text", text: "Working on it…" }, "working");
            this.interrupts.set(id, () =>
                this.publish(id, { ...done, is_error: true, result: "interrupted" }, "idle"),
            );
        } else if (text.startsWith("run:")) {
            this.askPermission(id, text.slice(4).trim(), done);
        } else {
            // Written word by word, the way a real reply arrives.
            const reply = `You said: ${text}`;
            const words = reply.split(/(?<= )/);
            words.forEach((word, index) => {
                this.stream(id, { type: "text_delta", text: word }, (index + 1) / words.length);
            });
            this.later(id, { type: "text", text: reply }, "working");
            this.later(id, done, "idle", 2);
            this.later(id, SAMPLE_CONTEXT, null, 3);
        }
    }

    private askPermission(id: number, command: string, done: AgentEvent) {
        const input = { command };
        const toolId = `tool-${this.nextId++}`;
        this.later(id, { type: "tool_use", id: toolId, name: "Bash", input }, "working");
        const request: AgentEvent = {
            type: "permission_request",
            request_id: `request-${toolId}`,
            tool_name: "Bash",
            input,
            description: null,
        };
        this.later(id, request, "waiting", 2);
        this.decisions.set(id, (allowed) => {
            const content = allowed ? "ok" : "The user declined this action.";
            const result: AgentEvent = {
                type: "tool_result",
                tool_use_id: toolId,
                content,
                is_error: !allowed,
            };
            // The command takes a while, as real ones do.
            this.later(id, result, "working", 3);
            this.later(
                id,
                { type: "text", text: allowed ? "Done." : "I won't run that." },
                "working",
                4,
            );
            this.later(id, done, "idle", 5);
        });
    }

    decide(id: number, requestId: string, allowed: boolean) {
        const decision = this.decisions.get(id);
        this.decisions.delete(id);
        const answered: AgentEvent = {
            type: "permission_answered",
            request_id: requestId,
            allowed,
        };
        this.publish(id, answered, "working");
        if (decision) {
            decision(allowed);
        } else {
            // The request shown in the sample transcript: just end the turn.
            const content = allowed ? "Everything up-to-date" : "The user declined this action.";
            this.publish(
                id,
                { type: "tool_result", tool_use_id: "t5", content, is_error: !allowed },
                "working",
            );
            this.later(
                id,
                { type: "turn_done", is_error: false, result: null, cost_usd: 0.31 },
                "idle",
            );
        }
    }

    private tasks() {
        return this.projects.flatMap((project) => project.tasks);
    }

    configure(id: number, settings: Partial<SessionSettings>): Session {
        const sessions = this.tasks().flatMap((task) => task.sessions);
        const found = sessions.find((candidate) => candidate.id === id);
        if (!found) {
            throw new Error(`session ${id} not found`);
        }
        return Object.assign(found, settings);
    }

    rename(id: number, title: string): Session {
        const renamed = this.configure(id, {}) as Session;
        renamed.title = title.trim();
        this.publish(id, { type: "renamed", title: renamed.title }, null);
        return renamed;
    }

    close(id: number) {
        this.interrupt(id);
        for (const task of this.tasks()) {
            task.sessions = task.sessions.filter((candidate) => candidate.id !== id);
        }
    }

    interrupt(id: number) {
        this.interrupts.get(id)?.();
        this.interrupts.delete(id);
    }
}

/** An in-memory stand-in for the app. */
export function createMockApi(): Api {
    const agent = new MockAgent();
    const tasks = () => agent.projects.flatMap((project) => project.tasks);
    const hookListeners = new Set<(output: HookOutput) => void>();
    const terminal = new MockTerminal();
    const keys: ApiKeyInfo[] = [
        { name: "OPENROUTER_API_KEY", provider: "OpenRouter", stored: false },
        { name: "OPENAI_API_KEY", provider: "OpenAI", stored: true },
        { name: "ANTHROPIC_API_KEY", provider: "Anthropic API", stored: false },
        { name: "GEMINI_API_KEY", provider: "Google Gemini", stored: false },
    ];
    let settings: Settings = {
        editor: "code",
        auto_title: true,
        theme: "system",
        text_scale: 100,
        default_supervision: "auto",
        html_views: true,
        status_items: ["branch", "changes", "model", "context", "cost"],
        status_command: "",
    };
    return {
        overview: async () => structuredClone(agent.projects),
        agents: async () => [
            { id: "claude", name: "Claude Code", models: ["opus", "sonnet", "haiku", "fable"] },
            {
                id: "opencode",
                name: "OpenCode",
                models: ["openrouter/openai/gpt-5.6", "openrouter/google/gemini-3-pro"],
            },
        ],
        pickFolder: async () => "/Users/you/code/new-repo",
        projectAdd: async (path) => {
            const name = path.split("/").pop() ?? "repo";
            if (agent.projects.some((project) => project.name === name)) {
                throw new Error(`project \`${name}\` already exists`);
            }
            const project = { id: agent.projects.length + 1, name, repo_path: path, tasks: [] };
            agent.projects.push(project);
            return project;
        },
        branchChoices: async (projectName) => ({
            base: "origin/main",
            existing: projectName === "hero" ? ["feature/vat-export", "pm-16001-invoices"] : [],
        }),
        taskNew: async (projectName, name, branch) => {
            const project = agent.projects.find((candidate) => candidate.name === projectName);
            if (!project) {
                throw new Error(`project \`${projectName}\` not found`);
            }
            for (const line of ["Installing dependencies", "Seeding the database"]) {
                for (const listener of hookListeners) {
                    listener({ project: projectName, task: name, line });
                }
                await new Promise((resolve) => setTimeout(resolve, PACE * 2));
            }
            const created = {
                ...task(Date.now(), project.id, name, []),
                ...(branch ? { branch } : {}),
            };
            project.tasks.push(created);
            return created;
        },
        taskChanges: sampleChanges,
        taskDiff: sampleDiff,
        taskOpenEditor: async () => undefined,
        taskOpenUrl: async () => undefined,
        apiKeys: async () => structuredClone(keys),
        setApiKey: async (name, key) => storeKey(keys, name, key.trim() !== ""),
        deleteApiKey: async (name) => storeKey(keys, name, false),
        settings: async () => ({ ...settings }),
        setSettings: async (changed) => {
            settings = changed;
        },
        onHookOutput: async (listener) => {
            hookListeners.add(listener);
            return () => hookListeners.delete(listener);
        },
        sessionNew: async (_project, taskName, setup) => {
            const home = tasks().find((candidate) => candidate.name === taskName);
            if (!home) {
                throw new Error(`task \`${taskName}\` not found`);
            }
            const created = {
                ...agent.newSession(home.id),
                ...setup,
                agent: setup.agent ?? "claude",
            };
            home.sessions.push(created);
            return created;
        },
        sessionEvents: async (id) => [...agent.history(id)],
        sessionSend: async (id, text) => agent.send(id, text),
        sessionAllow: async (id, requestId) => agent.decide(id, requestId, true),
        sessionDeny: async (id, requestId) => agent.decide(id, requestId, false),
        sessionInterrupt: async (id) => agent.interrupt(id),
        slashCommands: async () => [
            "code-review",
            "context",
            "frontend-design",
            "grill-me",
            "init",
            "planning",
            "review",
            "security-review",
            "simplify",
            "tdd",
        ],
        sessionConfigure: async (id, settings) => agent.configure(id, settings),
        // A browser has no custom scheme; a blob address is enough to see the view.
        viewUrl: async (document) =>
            URL.createObjectURL(new Blob([document], { type: "text/html" })),
        terminalOpen: async (id) => terminal.open(id),
        terminalWrite: async (id, data) => terminal.type(id, data),
        terminalResize: async () => undefined,
        onTerminalOutput: async (listener) => {
            terminal.listeners.add(listener);
            return () => terminal.listeners.delete(listener);
        },
        statusLine: async (id) => scriptedStatus(settings.status_command, id),
        sessionRename: async (id, title) => agent.rename(id, title),
        sessionClose: async (id) => agent.close(id),
        onSessionEvent: async (listener) => {
            agent.listeners.add(listener);
            return () => agent.listeners.delete(listener);
        },
        onEventsLagged: async () => () => undefined,
    };
}

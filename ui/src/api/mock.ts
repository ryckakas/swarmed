import type { Api } from "./client";
import type {
    AgentEvent,
    HookOutput,
    Project,
    Session,
    SessionEvent,
    SessionStatus,
    Task,
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
            this.later(id, { type: "text", text: `You said: ${text}` }, "working");
            this.later(id, done, "idle", 2);
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
            this.publish(id, result, "working");
            this.later(
                id,
                { type: "text", text: allowed ? "Done." : "I won't run that." },
                "working",
            );
            this.later(id, done, "idle", 2);
        });
    }

    decide(id: number, allowed: boolean) {
        const decision = this.decisions.get(id);
        this.decisions.delete(id);
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
        sessionAllow: async (id) => agent.decide(id, true),
        sessionDeny: async (id) => agent.decide(id, false),
        sessionInterrupt: async (id) => agent.interrupt(id),
        onSessionEvent: async (listener) => {
            agent.listeners.add(listener);
            return () => agent.listeners.delete(listener);
        },
        onEventsLagged: async () => () => undefined,
    };
}

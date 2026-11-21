import { lastDays } from "../analytics";
import { PLAN_TOOL, withFiles } from "../transcript";
import { titleFrom } from "../tree";
import type { Api } from "./client";
import type {
    Action,
    AgentEvent,
    AgentSetup,
    ApiKeyInfo,
    BackgroundTask,
    Check,
    CloudProvider,
    CloudTask,
    Costs,
    CustomLayout,
    Environment,
    FileChange,
    FileDiff,
    FileListing,
    HookOutput,
    MarkColour,
    McpServerView,
    Project,
    Pull,
    PullDraft,
    QueuedMessage,
    SearchHit,
    Session,
    SessionEvent,
    SessionStatus,
    Settings,
    Skill,
    Snapshot,
    StackState,
    Task,
    Template,
    TerminalConfig,
    TerminalOpened,
    TerminalOutput,
    TurnStats,
    Unpublished,
    Usage,
} from "./types";

// Sample data and a scripted agent, for working on the UI in a browser where there is no Tauri
// shell. The agent behaves like the core's FakeBackend: plain text is echoed, `run: <command>`
// asks permission first, `ask` puts questions, `delegate` starts sub-agents, `show` answers
// with an HTML view, and `slow` runs until interrupted.

// `show` answers with this view: a chart in the app's colours, with buttons that redraw it.
const SAMPLE_VIEW = `<style>
.chart { max-width: 560px; }
.head { display: flex; justify-content: space-between; align-items: baseline; gap: 12px; }
.head h3 { margin: 0 0 12px; font-size: 1em; }
.pick button { border: 1px solid var(--line); border-radius: 6px; background: none;
    padding: 2px 10px; color: var(--muted); cursor: pointer; }
.pick button.on { color: var(--text); border-color: var(--accent); }
.bars { display: flex; align-items: flex-end; gap: 12px; height: 150px;
    border-bottom: 1px solid var(--line); }
.bar { flex: 1; display: flex; flex-direction: column; align-items: center;
    justify-content: flex-end; height: 100%; }
.bar b { font-weight: normal; font-size: 0.8em; color: var(--muted); margin-bottom: 4px; }
.bar i { width: 100%; background: var(--accent); border-radius: 4px 4px 0 0;
    transition: height 0.4s ease; }
.months { display: flex; gap: 12px; margin-top: 6px; }
.months span { flex: 1; text-align: center; font-size: 0.8em; color: var(--muted); }
</style>
<div class="chart">
  <div class="head">
    <h3>Invoiced per month, in €k</h3>
    <div class="pick"><button class="on" data-by="net">Net</button> <button data-by="gross">With VAT</button></div>
  </div>
  <div class="bars" id="bars"></div>
  <div class="months" id="months"></div>
</div>
<script>
const totals = { net: [42, 48, 45, 57, 61, 66], gross: [50, 57, 54, 68, 73, 79] };
const months = ["Jun", "Jul", "Aug", "Sep", "Oct", "Nov"];
const bars = document.getElementById("bars");
document.getElementById("months").innerHTML = months.map((m) => "<span>" + m + "</span>").join("");
bars.innerHTML = months.map(() => '<div class="bar"><b></b><i></i></div>').join("");
function draw(by) {
  const top = Math.max(...totals.gross);
  [...bars.children].forEach((bar, n) => {
    bar.querySelector("b").textContent = totals[by][n];
    bar.querySelector("i").style.height = (totals[by][n] / top) * 85 + "%";
  });
  document.querySelectorAll(".pick button").forEach((b) => b.classList.toggle("on", b.dataset.by === by));
}
document.querySelectorAll(".pick button").forEach((b) => (b.onclick = () => draw(b.dataset.by)));
draw("net");
</script>`;

// `ask` puts these questions to the user, `ask one` only the first.
const SAMPLE_QUESTIONS = [
    {
        question: "Which database should the service use?",
        header: "Database",
        multiSelect: false,
        options: [
            { label: "PostgreSQL", description: "What the other services already run on." },
            { label: "SQLite", description: "One file, nothing to run." },
            { label: "Keep it in memory", description: "Fine for a prototype, gone on restart." },
        ],
    },
    {
        question: "Which checks should run before a merge?",
        header: "Checks",
        multiSelect: true,
        options: [
            { label: "Unit tests", description: "" },
            { label: "Lint", description: "Formatting and the lint rules." },
            { label: "End-to-end tests", description: "Slow, and catches the most." },
        ],
    },
];

const SAMPLE_AGENTS: Record<number, string> = { 2: "codex", 4: "opencode" };

function session(id: number, taskId: number, title: string, status: SessionStatus): Session {
    return {
        id,
        task_id: taskId,
        // Not all on one agent, so the sample shows the marks that tell them apart.
        agent: SAMPLE_AGENTS[id] ?? "claude",
        title,
        agent_session_id: `mock-${id}`,
        model: null,
        supervision: "ask",
        effort: null,
        status,
        // One sample session has answered and not been looked at.
        unread: id === 5,
        spawned_by: null,
        fast: false,
        // One sample session carries a mark, as a skill's last step would leave it.
        mark: id === 3 ? { colour: "green", label: "e2e passed" } : null,
    };
}

function task(id: number, projectId: number, name: string, sessions: Session[]): Task {
    return {
        id,
        project_id: projectId,
        name,
        branch: `you/${name}`,
        base_ref: "origin/main",
        worktree_path: `/Users/you/swarmed/storefront/${name}`,
        slot: id,
        setup: "ok",
        archived: false,
        sessions,
        running: sessions.filter((s) => s.status !== "idle").map((s) => s.id),
        url: `http://localhost:${3000 + id * 100}`,
        urls: [`http://localhost:${3000 + id * 100}`],
        has_note: id === 1,
        closed: 0,
        environment: null,
        // One sample task has a dev server to start and stop.
        stack: id === 1 ? "running" : null,
    };
}

const noteKey = (project: string, task: string | null) => `${project}/${task ?? ""}`;

/** The notes the sample data starts with. */
const notes = new Map<string, string>([
    [
        noteKey("storefront", null),
        "Staging: https://staging.example.test\nDeploys go out on Thursdays.\n",
    ],
    [noteKey("storefront", "fix-login-redirect"), "Test user: demo@example.test\n"],
]);

/** Marks in the tree whether a repository or a task has a note. */
function markNote(projects: Project[], project: string, task: string | null, has: boolean) {
    const home = projects.find((candidate) => candidate.name === project);
    const target = task === null ? home : home?.tasks.find((candidate) => candidate.name === task);
    if (target) {
        target.has_note = has;
    }
}

/** Removing repositories and tasks from the tree `projects` returns. */
function removeApi(projects: () => Project[]): Pick<Api, "projectRemove" | "taskRemove"> {
    return {
        projectRemove: async (name) => {
            const at = projects().findIndex((project) => project.name === name);
            projects().splice(at, at === -1 ? 0 : 1);
            return [];
        },
        taskRemove: async (projectName, name, force) => {
            // One sample task holds unsaved work, to show what a refusal looks like.
            if (name === "invoice-export" && !force) {
                throw new Error("task `invoice-export` has uncommitted changes");
            }
            const tasks = projects().find((project) => project.name === projectName)?.tasks ?? [];
            const at = tasks.findIndex((task) => task.name === name);
            tasks.splice(at, at === -1 ? 0 : 1);
            return [];
        },
    };
}

/** MCP servers: a few of Claude Code's own to look at, and a list of Swarmed's to fill. */
function mcpApi(): Pick<Api, "mcpServers" | "mcpSave" | "mcpRemove"> {
    const claude = (name: string, status: string, target: string): McpServerView => ({
        name,
        origin: "claude",
        kind: target.startsWith("http") ? "http" : "stdio",
        target,
        secrets: [],
        enabled: true,
        status,
    });
    const fromClaude = [
        claude("context7", "connected", "https://mcp.context7.com/mcp"),
        claude("playwright", "connected", "npx @playwright/mcp@latest"),
        claude("claude.ai Slack", "needs-auth", ""),
        // Codex's own, from its configuration: it does not say whether they connect.
        {
            ...claude("linear", "", "npx -y @acme/linear-mcp"),
            origin: "codex",
            status: null,
            secrets: ["LINEAR_TOKEN"],
        },
        {
            ...claude("docs", "", "https://example.com/mcp"),
            origin: "codex",
            status: null,
            enabled: false,
        },
    ];
    let own: McpServerView[] = [];
    const all = () => [...own, ...fromClaude];
    return {
        mcpServers: async () => all(),
        mcpSave: async (server) => {
            const view: McpServerView = {
                name: server.name,
                origin: "swarmed",
                kind: server.url === "" ? "stdio" : "http",
                target: server.url || server.command,
                secrets: server.secrets.map(([name]) => name),
                enabled: server.enabled,
                status: null,
            };
            own = [...own.filter((other) => other.name !== server.name), view];
            return all();
        },
        mcpRemove: async (name) => {
            own = own.filter((server) => server.name !== name);
            return all();
        },
    };
}

const SAMPLE_USAGE: Usage = {
    database_bytes: 4_700_000,
    events: 576,
    notes_bytes: 4100,
    worktrees: [
        {
            project: "storefront",
            task: "invoice-export",
            path: "/w/storefront/a",
            bytes: 365_600_000,
        },
        {
            project: "storefront",
            task: "fix-login-redirect",
            path: "/w/storefront/b",
            bytes: 282_800_000,
        },
        { project: "bonsai-lint", task: "rust-support", path: "/w/b/c", bytes: 540_700 },
        { project: "storefront", task: "shop-142", path: "/w/storefront/d", bytes: null },
    ],
    removed_bytes: 0,
    running: { processes: 9, memory_bytes: 612_000_000, cpu_percent: 14.2 },
    sessions: [
        {
            session_id: 2,
            title: "Write a regression test for the loop",
            processes: 4,
            memory_bytes: 410_000_000,
            cpu_percent: 12.6,
        },
    ],
};

/** Setting up an agent: the sample starts with OpenCode not installed, and "installs" it. */
function setupApi(
    hookListeners: Set<(output: HookOutput) => void>,
): Pick<
    Api,
    "agentSetups" | "agentInstall" | "agentConfigure" | "agentModelsRefresh" | "openrouterModels"
> {
    const hint = "To sign in, run its program in a terminal once.";
    const setups: AgentSetup[] = [
        {
            id: "claude",
            name: "Claude Code",
            about: "Anthropic's coding agent, on your Claude subscription. What sessions run on unless you choose otherwise.",
            installed: "/Users/you/.local/bin/claude",
            configured: true,
            models: [],
            install_command: "curl -fsSL https://claude.ai/install.sh | bash",
            sign_in:
                "To sign in, run `claude` in a terminal once and follow /login. Swarmed uses that login and never sees it.",
            built_in: true,
            model_source: "built_in",
        },
        {
            id: "codex",
            name: "Codex",
            about: "OpenAI's coding agent, on your ChatGPT plan or an OpenAI API key.",
            installed: null,
            configured: false,
            models: [],
            install_command: "brew install codex-acp",
            sign_in: hint,
            built_in: false,
            model_source: "agent",
        },
        {
            id: "opencode",
            name: "OpenCode",
            about: "An open-source coding agent that runs models from OpenRouter and other providers.",
            installed: null,
            configured: false,
            models: [],
            install_command: "brew install anomalyco/tap/opencode-v2",
            sign_in: "It uses the API keys stored in Settings.",
            built_in: false,
            model_source: "openrouter",
        },
    ];
    const setup = (id: string): AgentSetup => {
        const found = setups.find((candidate) => candidate.id === id);
        if (!found) {
            throw new Error(`agent \`${id}\` is not configured`);
        }
        return found;
    };
    return {
        agentSetups: async () => setups.map((each) => ({ ...each })),
        agentInstall: async (id) => {
            const installing = setup(id);
            for (const line of ["==> Fetching", "==> Pouring", "Installed"]) {
                for (const listener of hookListeners) {
                    listener({ project: "", task: id, line });
                }
                await new Promise((resolve) => setTimeout(resolve, PACE * 2));
            }
            const models =
                id === "opencode"
                    ? ["openrouter/openai/gpt-6.2-sol", "openrouter/google/gemini-3.8-flash"]
                    : ["gpt-6.2-sol", "gpt-6.2-astra", "gpt-6.1-sol"];
            Object.assign(installing, { installed: `/opt/homebrew/bin/${id}`, models });
            installing.configured = true;
            return { ...installing };
        },
        agentModelsRefresh: async (id) => {
            const asked = setup(id);
            if (asked.model_source === "agent") {
                asked.models = ["gpt-6.2-sol", "gpt-6.2-astra", "gpt-6.1-sol"];
            }
            return { ...asked };
        },
        agentConfigure: async (id, models) => {
            const configured = setup(id);
            configured.configured = true;
            configured.models = models ?? configured.models;
            return { ...configured };
        },
        openrouterModels: async () => [
            "openrouter/openai/gpt-6.2-sol",
            "openrouter/google/gemini-3.8-flash",
            "openrouter/x-ai/grok-4.7",
            "openrouter/deepseek/deepseek-v4.1-flash",
        ],
    };
}

/** Reading and writing notes, over the tree `projects` returns. */
function noteApi(projects: () => Project[]): Pick<Api, "note" | "setNote"> {
    return {
        note: async (project, task) => notes.get(noteKey(project, task)) ?? "",
        setNote: async (project, task, text) => {
            const kept = text.trim() === "" ? "" : text;
            notes.set(noteKey(project, task), kept);
            markNote(projects(), project, task, kept !== "");
        },
    };
}

/** A cloud task as each provider would have started it. */
function sampleCloudTask(projectId: number, prompt: string, provider: CloudProvider): CloudTask {
    const id = Date.now();
    const started = {
        codex: {
            remote_id: `task_${id}`,
            url: `https://chatgpt.com/codex/tasks/task_${id}`,
            status: "pending",
        },
        claude: {
            remote_id: `session_${id}`,
            url: `https://claude.ai/code/session_${id}`,
            status: "sent",
        },
    }[provider];
    return {
        id,
        project_id: projectId,
        provider,
        prompt,
        branch: "main",
        session_id: null,
        ...started,
    };
}

/** A session's conversation handed to the cloud: a task under what it was asked to do. */
function handedOn(
    projects: Project[],
    id: number,
    instruction: string,
    provider: CloudProvider,
): CloudTask {
    const holds = (project: Project) =>
        project.tasks.some((task) => task.sessions.some((session) => session.id === id));
    const project = projects.find(holds);
    if (!project) {
        throw new Error(`session ${id} not found`);
    }
    const asked = instruction.trim() || "Carry on with the work this conversation arrived at.";
    const task = sampleCloudTask(project.id, asked, provider);
    project.cloud = [task, ...project.cloud];
    return task;
}

/** Delegating to the cloud: the sample has an environment for `storefront` and for nothing else. */
function cloudApi(
    projects: () => Project[],
): Pick<
    Api,
    | "cloudDelegate"
    | "cloudDelegateConversation"
    | "cloudTasks"
    | "cloudTaskForget"
    | "cloudTaskOpen"
    | "cloudTaskDiff"
    | "cloudTaskApply"
> {
    const find = (name: string) => {
        const project = projects().find((candidate) => candidate.name === name);
        if (!project) {
            throw new Error(`project \`${name}\` not found`);
        }
        return project;
    };
    return {
        cloudDelegate: async (name, prompt, _task, provider) => {
            const project = find(name);
            if (provider === "codex" && name !== "storefront") {
                throw new Error(
                    `you/${name} has no Codex cloud environment. Create one for it at https://chatgpt.com/codex, under Environments, then delegate again`,
                );
            }
            const task = sampleCloudTask(project.id, prompt, provider);
            project.cloud = [task, ...project.cloud];
            return task;
        },
        cloudDelegateConversation: async (id, instruction, provider) =>
            handedOn(projects(), id, instruction, provider),
        cloudTasks: async (name, refresh) => {
            const project = find(name);
            if (refresh) {
                project.cloud = project.cloud.map((task) =>
                    task.provider === "codex" ? { ...task, status: "ready" } : task,
                );
            }
            return project.cloud;
        },
        cloudTaskForget: async (id) => {
            for (const project of projects()) {
                project.cloud = project.cloud.filter((task) => task.id !== id);
            }
        },
        cloudTaskOpen: async () => undefined,
        cloudTaskDiff: async () => SAMPLE_CLOUD_DIFF,
        cloudTaskApply: async () => undefined,
    };
}

/** Snapshots of the sample projects: none to begin with, kept as they are taken. */
function snapshotApi(
    projects: () => Project[],
): Pick<Api, "snapshots" | "snapshotSave" | "snapshotRemove"> {
    const kept = new Map<string, Snapshot[]>();
    const of = (project: string) => kept.get(project) ?? [];
    return {
        snapshots: async (project) => [...of(project)],
        snapshotSave: async (project, name) => {
            const tasks = projects().find((one) => one.name === project)?.tasks ?? [];
            const environment = tasks.find((task) => task.name === name)?.environment ?? null;
            const snapshot = {
                environment,
                task: name,
                taken_at: Math.floor(Date.now() / 1000),
                entries: 3,
            };
            const others = of(project).filter((one) => one.environment !== environment);
            kept.set(project, [...others, snapshot]);
            return snapshot;
        },
        snapshotRemove: async (project, environment) => {
            kept.set(
                project,
                of(project).filter((one) => one.environment !== environment),
            );
        },
    };
}

/** A few weeks of made-up work on three models, ending today, with two quiet days. */
function sampleAnalytics(today: Date): TurnStats[] {
    const models: [agent: string, model: string, usd: number][] = [
        ["claude", "claude-opus-5-5", 2.4],
        ["claude", "claude-haiku-5-5", 0.3],
        ["codex", "gpt-6.1-sol", 0.9],
    ];
    return lastDays(today, 24).flatMap((day, index) =>
        models
            .filter((_, at) => index % 7 !== 5 && (index + at) % 4 !== 0)
            .map(([agent, model, usd], at): TurnStats => {
                const busy = 1 + ((index * 7 + at * 3) % 5);
                return {
                    day,
                    project: "storefront",
                    task: "fix-login-redirect",
                    agent,
                    model,
                    turns: busy * 3,
                    failed: busy === 4 ? 1 : 0,
                    usd: usd * busy,
                    duration_ms: busy * 190_000,
                };
            }),
    );
}

/** Things the sample agents left running: one still going, two that ended. */
function backgroundApi(): Pick<Api, "taskBackground" | "sessionStopBackground"> {
    const tasks: BackgroundTask[] = [
        {
            session_id: 1,
            session_title: "Fix the redirect loop after login",
            task_id: "b1",
            description: "Run the dev server on port 3100",
            kind: "local_bash",
            state: "running",
            summary: null,
        },
        {
            session_id: 1,
            session_title: "Fix the redirect loop after login",
            task_id: "b2",
            description: "Run the whole test suite",
            kind: "local_bash",
            state: "completed",
            summary: 'Background command "Run the whole test suite" completed (exit code 0)',
        },
        {
            session_id: 2,
            session_title: "Write a regression test for the loop",
            task_id: "b3",
            description: "Look for other callers of the redirect helper",
            kind: "local_agent",
            state: "failed",
            summary: "The agent stopped with an error",
        },
    ];
    return {
        taskBackground: async (_project, name) =>
            name === "fix-login-redirect" ? tasks.map((task) => ({ ...task })) : [],
        sessionStopBackground: async (id, taskId) => {
            const task = tasks.find((one) => one.session_id === id && one.task_id === taskId);
            if (task) {
                task.state = "stopped";
            }
        },
    };
}

/** Starting and stopping what a sample task keeps running. */
function stackApi(projects: () => Project[]): Pick<Api, "stackStart" | "stackStop" | "stackLog"> {
    const set = (project: string, name: string, state: StackState) => {
        const tasks = projects().find((candidate) => candidate.name === project)?.tasks ?? [];
        const task = tasks.find((candidate) => candidate.name === name);
        if (task && task.stack !== null) {
            task.stack = state;
            // A sample stack takes a moment to answer, as a real one does.
            if (state === "starting") {
                setTimeout(() => {
                    task.stack = task.stack === "starting" ? "running" : task.stack;
                }, 6000);
            }
        }
        return task?.stack ?? null;
    };
    return {
        stackStart: async (project, name) => set(project, name, "starting") !== null,
        stackStop: async (project, name) => void set(project, name, "stopped"),
        stackLog: async () => "Starting dev server…\nListening on http://localhost:3100\n",
    };
}

/** Skills: a few of each agent's to look at, and Swarmed's own to fill. */
function skillApi(): Pick<
    Api,
    "skills" | "skillBody" | "skillSave" | "skillRemove" | "skillShare"
> {
    const of = (origin: string, name: string, description: string): Skill => ({
        origin,
        name,
        description,
        folder: `/Users/you/.${origin}/skills/${name}`,
        shared: true,
    });
    let skills: Skill[] = [
        of("claude", "code-review", "Review a change for correctness and readability."),
        of("claude", "bug-hunter", "Investigate a defect before proposing a fix."),
        of("codex", "deploy", "Ship the current branch. Use when the user says deploy."),
        of("agents", "triage", "Sort incoming bug reports by severity."),
    ];
    const bodies = new Map<string, string>();
    const own = (name: string) => (skill: Skill) =>
        skill.origin === "swarmed" && skill.name === name;
    return {
        skills: async () => skills,
        skillBody: async (name) => bodies.get(name) ?? "",
        skillSave: async (name, description, body) => {
            if (!/^[a-z0-9][a-z0-9._-]*$/.test(name)) {
                throw new Error(`invalid name \`${name}\``);
            }
            bodies.set(name, body);
            const saved = {
                ...of("swarmed", name, description.trim()),
                folder: `/data/skills/${name}`,
            };
            skills = [saved, ...skills.filter((skill) => !own(name)(skill))];
            return skills;
        },
        skillRemove: async (name) => {
            skills = skills.filter((skill) => !own(name)(skill));
            return skills;
        },
        skillShare: async (origin, name, shared) => {
            skills = skills.map((skill) =>
                skill.origin === origin && skill.name === name ? { ...skill, shared } : skill,
            );
            return skills;
        },
    };
}

/** Environments, kept per repository for as long as the page is open. */
function environmentApi(): Pick<Api, "environments" | "environmentSave" | "environmentRemove"> {
    const kept = new Map<string, Record<string, Environment>>([
        [
            "storefront",
            {
                staging: {
                    setup: "npm ci && npm run db:seed -- --staging",
                    start: "npm run dev",
                    stop: "",
                    teardown: "",
                    open_url: "http://localhost:{{port.web}}",
                    ports: { web: 3000 },
                    env: { APP_ENV: "staging", DATABASE_URL: "postgres://localhost/{{task}}" },
                },
            },
        ],
    ]);
    const of = (project: string) => kept.get(project) ?? {};
    return {
        environments: async (project) => ({ ...of(project) }),
        environmentSave: async (project, name, environment) => {
            if (!/^[a-z0-9][a-z0-9._-]*$/.test(name)) {
                throw new Error(`invalid name \`${name}\``);
            }
            kept.set(project, { ...of(project), [name]: environment });
        },
        environmentRemove: async (project, name) => {
            const { [name]: _removed, ...rest } = of(project);
            kept.set(project, rest);
        },
    };
}

export function sampleProjects(): Project[] {
    return [
        {
            id: 1,
            name: "storefront",
            repo_path: "/Users/you/code/storefront",
            has_note: true,
            cloud: [
                {
                    id: 1,
                    project_id: 1,
                    provider: "codex",
                    remote_id: "task_sample",
                    url: "https://chatgpt.com/codex/tasks/task_sample",
                    prompt: "Bring the API docs up to date with the new VAT fields",
                    branch: "main",
                    status: "ready",
                    session_id: null,
                },
            ],
            tasks: [
                task(1, 1, "fix-login-redirect", [
                    session(1, 1, "Fix the redirect loop after login", "waiting"),
                    session(2, 1, "Write a regression test for the loop", "working"),
                ]),
                task(2, 1, "invoice-export", [
                    session(3, 2, "Why does the CSV export drop the VAT column?", "idle"),
                ]),
                task(3, 1, "shop-142", []),
            ],
        },
        {
            id: 2,
            name: "bonsai-lint",
            repo_path: "/Users/you/code/bonsai-lint",
            has_note: false,
            cloud: [],
            tasks: [
                task(4, 2, "rust-support", [
                    session(4, 4, "Sketch a Rust language spec", "errored"),
                    session(5, 4, "A very long conversation", "idle"),
                ]),
            ],
        },
        {
            id: 3,
            name: "fsum",
            repo_path: "/Users/you/code/fsum",
            tasks: [],
            has_note: false,
            cloud: [],
        },
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
        { name: "postgres", status: "failed" },
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

/** One sample task has work to commit, another a branch waiting for a pull request. */
async function sampleUnpublished(_project: string, name: string): Promise<Unpublished> {
    return {
        changed: name === "fix-login-redirect" ? 4 : 0,
        unpushed: 0,
        pushed_before: name === "invoice-export",
    };
}

const SAMPLE_DRAFT: PullDraft = {
    title: "Export invoices as CSV",
    body: "- Add the exporter\n- Quote fields that hold a separator",
};

async function sampleOpenedPull(): Promise<Pull> {
    const pull = await samplePull("", "fix-login-redirect");
    if (pull === null) {
        throw new Error("the sample pull request is gone");
    }
    return pull;
}

/** One sample task has a pull request with a failed check; the others have none. */
async function samplePull(_project: string, name: string): Promise<Pull | null> {
    if (name !== "fix-login-redirect") {
        return null;
    }
    return {
        number: 412,
        title: "Fix the redirect loop after login",
        url: "https://github.com/example/storefront/pull/412",
        state: "open",
        draft: false,
        review: "review_required",
        checks: [
            {
                name: "Lint",
                workflow: "CI",
                outcome: "passed",
                url: "https://github.com/example/storefront/runs/1",
            },
            {
                name: "Unit tests",
                workflow: "CI",
                outcome: "failed",
                url: "https://github.com/example/storefront/runs/2",
            },
            { name: "chromium (1/2)", workflow: "E2E", outcome: "running", url: null },
            { name: "chromium (2/2)", workflow: "E2E", outcome: "passed", url: null },
            { name: "Deploy preview", workflow: null, outcome: "skipped", url: null },
            // A matrix, as large repositories have: enough to need narrowing down.
            ...["18", "20", "22"].flatMap((node): Check[] =>
                ["macos", "ubuntu"].map((system) => ({
                    name: `Build (${system}, node ${node})`,
                    workflow: "CI",
                    outcome: "passed",
                    url: null,
                })),
            ),
        ],
    };
}

async function sampleChanges(_project: string, name: string): Promise<FileChange[]> {
    return name === "shop-142" ? [] : SAMPLE_CHANGES;
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

/** The sixteen colours by name, each in itself, and an address to click: what a fresh shell shows. */
const COLOUR_NAMES = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
const SWATCHES = [30, 90]
    .map((first) =>
        COLOUR_NAMES.map((name, at) => `\u001b[${first + at}m${name}\u001b[0m`).join(" "),
    )
    .join("\r\n");
const GREETING = `${SWATCHES}\r\nDocs at https://example.com/docs\r\n`;

/** A small worktree to walk through: each folder by its path, with folders ending in a slash. */
const SAMPLE_TREE: Record<string, string[]> = {
    "": [".github/", "public/", "src/", "test/", ".gitignore", "README.md", "package.json"],
    ".github": ["workflows/"],
    ".github/workflows": ["ci.yml", "e2e.yml"],
    public: ["logo.png"],
    src: ["auth/", "main.ts"],
    "src/auth": ["login.ts", "safe-next.ts"],
    test: ["auth/"],
    "test/auth": ["login.test.ts"],
};

function sampleFiles(dir: string): FileListing {
    const names = SAMPLE_TREE[dir] ?? [];
    return {
        entries: names.map((name) => ({ name: name.replace(/\/$/, ""), dir: name.endsWith("/") })),
        more: 0,
    };
}

/** A pretend shell: it echoes what is typed and answers every command the same way. */
class MockTerminal {
    readonly listeners = new Set<(output: TerminalOutput) => void>();
    private readonly printed = new Map<number, string>();

    open(id: number): TerminalOpened {
        const replay = this.printed.get(id);
        if (replay === undefined) {
            // Marked as started at once, so opening it twice in a row starts one shell.
            this.printed.set(id, "");
            setTimeout(() => this.print(id, GREETING + PROMPT), 0);
        }
        return { replay: replay ?? "", end: replay?.length ?? 0 };
    }

    type(id: number, data: string) {
        const entered = data.includes("\r");
        this.print(id, entered ? `\r\n(the sample shell runs nothing)\r\n${PROMPT}` : data);
    }

    private print(id: number, data: string) {
        const before = this.printed.get(id) ?? "";
        this.printed.set(id, before + data);
        for (const listener of this.listeners) {
            listener({ session_id: id, data, offset: before.length });
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

/** A title as the core's namer might suggest it: here, the message's first words, capitalised. */
export function suggestedTitle(message: string): string {
    const words = titleFrom(message).split(/\s+/).slice(0, 6).join(" ");
    return words.charAt(0).toUpperCase() + words.slice(1);
}

class MockAgent {
    readonly projects = sampleProjects();
    readonly events = new Map<number, AgentEvent[]>();
    readonly listeners = new Set<Listener>();
    /** What to do when a session's pending permission request is answered. */
    private readonly decisions = new Map<
        number,
        (allowed: boolean, answers?: Record<string, string>) => void
    >();
    private readonly interrupts = new Map<number, () => void>();
    private nextId = 100;

    history(id: number): AgentEvent[] {
        let events = this.events.get(id);
        if (!events) {
            events = sampleEvents(id);
            // One sample session's agent has proposed a rule, which waits for an answer.
            if (id === 2) {
                events.push({
                    type: "config_proposed",
                    request_id: "config-sample",
                    title: "Add this rule for every agent?",
                    detail: "never use em dashes in anything you write",
                });
            }
            this.events.set(id, events);
        }
        return events;
    }

    publish(sessionId: number, event: AgentEvent, status: SessionStatus | null) {
        this.history(sessionId).push(event);
        this.record(sessionId, event, status);
        this.tell(sessionId, event, status);
        // A turn that ended well makes room for the next waiting message, as in the core.
        if (event.type === "turn_done" && !event.is_error) {
            this.sendQueued(sessionId);
        }
    }

    /**
     * Keeps what an event changes about its session, as the core does: the tree read again
     * must say what the events said, or a refresh would put a working session back to idle
     * and take a new session's title away.
     */
    private record(sessionId: number, event: AgentEvent, status: SessionStatus | null) {
        const session = this.tasks()
            .flatMap((task) => task.sessions)
            .find((candidate) => candidate.id === sessionId);
        // A closed session is in no task any more; what is left of its turn changes nothing.
        if (!session) {
            return;
        }
        session.status = status ?? session.status;
        if (event.type === "turn_done") {
            session.unread = true;
        } else if (event.type === "renamed") {
            session.title = event.title;
        } else if (event.type === "user_message" && session.title === "") {
            session.title = titleFrom(event.text);
        }
    }

    private tell(sessionId: number, event: AgentEvent, status: SessionStatus | null) {
        for (const listener of this.listeners) {
            listener({ session_id: sessionId, status, event });
        }
    }

    private readonly queues = new Map<number, QueuedMessage[]>();

    queue(id: number): QueuedMessage[] {
        return [...(this.queues.get(id) ?? [])];
    }

    /** Replaces a session's queue and tells the listeners, without making it history. */
    private setQueue(id: number, messages: QueuedMessage[]): QueuedMessage[] {
        this.queues.set(id, messages);
        this.tell(id, { type: "queue", messages }, null);
        return messages;
    }

    enqueue(id: number, text: string): QueuedMessage[] {
        return this.setQueue(id, [...this.queue(id), { id: this.nextId++, text }]);
    }

    editQueued(id: number, message: number, text: string): QueuedMessage[] {
        const edited = this.queue(id).map((queued) =>
            queued.id === message ? { ...queued, text } : queued,
        );
        return this.setQueue(
            id,
            edited.filter((queued) => queued.text.trim() !== ""),
        );
    }

    sendQueued(id: number) {
        const [first, ...rest] = this.queue(id);
        if (first) {
            this.setQueue(id, rest);
            this.send(id, first.text);
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
        // Nothing has been said to an agent yet, so which one it is can still be chosen.
        created.agent_session_id = null;
        this.events.set(created.id, []);
        return created;
    }

    send(id: number, text: string) {
        this.publish(id, { type: "user_message", text }, "working");
        const said = this.history(id).filter((event) => event.type === "user_message");
        const first = said.length === 1;
        const done: AgentEvent = {
            type: "turn_done",
            is_error: false,
            result: null,
            cost_usd: 0.0123,
            duration_ms: 72_000,
        };
        if (text === "slow") {
            this.later(id, { type: "text", text: "Working on it…" }, "working");
            this.interrupts.set(id, () =>
                this.publish(id, { ...done, is_error: true, result: "interrupted" }, "idle"),
            );
        } else if (this.configure(id, {}).supervision === "plan") {
            this.presentPlan(id, text, done);
        } else if (text === "delegate") {
            this.delegate(id, done);
        } else if (text.startsWith("ask")) {
            this.askQuestions(id, text === "ask one", done);
        } else if (text.startsWith("run:")) {
            this.askPermission(id, text.slice(4).trim(), done);
        } else if (text.startsWith("show")) {
            this.showView(id, done);
        } else {
            // Written word by word, the way a real reply arrives.
            const reply = `You said: ${text}`;
            const words = reply.split(/(?<= )/);
            words.forEach((word, index) => {
                this.stream(id, { type: "text_delta", text: word }, (index + 1) / words.length);
            });
            // A better title arrives while the first reply is still being written, as the
            // core's does: the reply so far must stay on screen through it.
            if (first) {
                this.later(id, { type: "renamed", title: suggestedTitle(text) }, null, 0.5);
            }
            this.later(id, { type: "text", text: reply }, "working");
            this.later(id, done, "idle", 2);
            this.later(id, SAMPLE_CONTEXT, null, 3);
            const { model, effort } = this.configure(id, {});
            const applied = { model: model ?? "claude-opus-5-5", effort: effort ?? "xhigh" };
            this.later(id, { type: "applied", ...applied }, null, 3);
            if (text.startsWith("suggest:")) {
                this.later(id, { type: "suggestion", text: text.slice(8).trim() }, null, 4);
            }
        }
    }

    /** Answers with an HTML view, written a line at a time as a real reply arrives. */
    private showView(id: number, done: AgentEvent) {
        const reply = `Here is what was invoiced in each of the last six months.\n\n\`\`\`html-view\n${SAMPLE_VIEW}\n\`\`\`\n\nNovember is the highest so far.`;
        const lines = reply.split(/(?<=\n)/);
        lines.forEach((line, index) => {
            this.stream(id, { type: "text_delta", text: line }, (3 * (index + 1)) / lines.length);
        });
        this.later(id, { type: "text", text: reply }, "working", 3);
        this.later(id, done, "idle", 4);
    }

    /** In plan mode the agent only proposes; approving the plan ends plan mode. */
    private presentPlan(id: number, text: string, done: AgentEvent) {
        const plan = `## ${text}\n\n1. Read the code this touches.\n2. Make the change in \`src/\`.\n3. Run the tests.\n\nNothing is changed until you approve.`;
        const toolId = this.ask(id, PLAN_TOOL, { plan });
        this.decisions.set(id, (approved) => {
            const content = approved ? "ok" : "The user wants changes to the plan.";
            const result: AgentEvent = {
                type: "tool_result",
                tool_use_id: toolId,
                content,
                is_error: !approved,
            };
            this.later(id, result, "working");
            if (approved) {
                this.configure(id, { supervision: "auto" });
                this.later(id, { type: "supervision_changed", supervision: "auto" }, null);
            }
            const reply = approved ? "Starting on the plan." : "What should I change?";
            this.later(id, { type: "text", text: reply }, "working", 2);
            this.later(id, done, "idle", 3);
        });
    }

    /** Calls a tool and asks for permission; returns the call's id. */
    private ask(id: number, toolName: string, input: unknown): string {
        const toolId = `tool-${this.nextId++}`;
        this.later(id, { type: "tool_use", id: toolId, name: toolName, input }, "working");
        const request: AgentEvent = {
            type: "permission_request",
            request_id: `request-${toolId}`,
            tool_name: toolName,
            input,
            description: null,
        };
        this.later(id, request, "waiting", 2);
        return toolId;
    }

    /** Starts two sub-agents, which report for a while and hand their answers back in turn. */
    private delegate(id: number, done: AgentEvent) {
        const start = (toolId: string, description: string): AgentEvent => ({
            type: "tool_use",
            id: toolId,
            name: "Agent",
            input: {
                description,
                subagent_type: "Explore",
                prompt: `Please ${description.toLowerCase()} and report what you find.`,
            },
        });
        const said = (parent: string, event: AgentEvent): AgentEvent => ({
            type: "sub",
            parent,
            event,
        });
        const [routes, tests] = [`sub-${this.nextId++}`, `sub-${this.nextId++}`];
        this.later(id, { type: "text", text: "I will look at this from two sides." }, "working");
        this.later(id, start(routes, "Find the routes"), "working", 2);
        this.later(id, start(tests, "Read the tests"), "working", 2);
        const grep: AgentEvent = {
            type: "tool_use",
            id: `${routes}-grep`,
            name: "Grep",
            input: { pattern: "router\\.(get|post)" },
        };
        const found: AgentEvent = {
            type: "tool_result",
            tool_use_id: `${routes}-grep`,
            content: "src/routes.ts:12\nsrc/routes.ts:40",
            is_error: false,
        };
        this.later(id, said(routes, { type: "text", text: "Searching for the routes." }), null, 4);
        this.later(id, said(routes, grep), null, 6);
        this.later(id, said(tests, { type: "text", text: "Reading the test folder." }), null, 7);
        this.later(id, said(routes, found), null, 9);
        this.later(
            id,
            said(routes, { type: "text", text: "Two routes, both in one file." }),
            null,
            11,
        );
        const answer = (toolId: string, content: string): AgentEvent => ({
            type: "tool_result",
            tool_use_id: toolId,
            content,
            is_error: false,
        });
        this.later(id, answer(routes, "There are two routes, in `src/routes.ts`."), "working", 20);
        this.later(id, answer(tests, "The tests cover one of them."), "working", 32);
        this.later(id, { type: "text", text: "Two routes, one of them tested." }, "working", 33);
        this.later(id, done, "idle", 34);
    }

    /** Puts questions to the user the way Claude Code does, and says back what was answered. */
    private askQuestions(id: number, one: boolean, done: AgentEvent) {
        const toolId = this.ask(id, "AskUserQuestion", {
            questions: one ? SAMPLE_QUESTIONS.slice(0, 1) : SAMPLE_QUESTIONS,
        });
        this.decisions.set(id, (allowed, answers) => {
            const said = Object.entries(answers ?? {}).map(
                ([asked, answer]) => `"${asked}"="${answer}"`,
            );
            const content = allowed
                ? `User has answered your questions: ${said.join(", ")}`
                : "The user chose not to answer these questions.";
            const result: AgentEvent = {
                type: "tool_result",
                tool_use_id: toolId,
                content,
                is_error: !allowed,
            };
            this.later(id, result, "working");
            const reply = allowed ? `Going with ${said.join(" and ")}.` : "I will decide myself.";
            this.later(id, { type: "text", text: reply }, "working", 2);
            this.later(id, done, "idle", 3);
        });
    }

    private askPermission(id: number, command: string, done: AgentEvent) {
        const toolId = this.ask(id, "Bash", { command });
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

    decide(id: number, requestId: string, allowed: boolean, answers?: Record<string, string>) {
        const decision = this.decisions.get(id);
        this.decisions.delete(id);
        const answered: AgentEvent = {
            type: "permission_answered",
            request_id: requestId,
            allowed,
        };
        this.publish(id, answered, "working");
        if (decision) {
            decision(allowed, answers);
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
                {
                    type: "turn_done",
                    is_error: false,
                    result: null,
                    cost_usd: 0.31,
                    duration_ms: 4_200,
                },
                "idle",
            );
        }
    }

    private tasks() {
        return this.projects.flatMap((project) => project.tasks);
    }

    configure(id: number, settings: Partial<Session>): Session {
        const sessions = this.tasks().flatMap((task) => task.sessions);
        const found = sessions.find((candidate) => candidate.id === id);
        if (!found) {
            throw new Error(`session ${id} not found`);
        }
        return Object.assign(found, settings);
    }

    /** Answers the sample proposal, in whichever session it was made. */
    answerProposal(requestId: string, accepted: boolean) {
        for (const [id, events] of this.events) {
            if (events.some((event) => "request_id" in event && event.request_id === requestId)) {
                this.publish(
                    id,
                    { type: "config_answered", request_id: requestId, accepted },
                    null,
                );
                this.publish(id, { type: "config_changed", kind: "rule" }, null);
            }
        }
    }

    mark(id: number, colour: MarkColour | "none", label: string): Session {
        const marked = this.configure(id, {}) as Session;
        marked.mark = colour === "none" ? null : { colour, label: label.trim() };
        this.publish(id, { type: "marked", colour: marked.mark?.colour ?? null, label }, null);
        return marked;
    }

    rename(id: number, title: string): Session {
        const renamed = this.configure(id, {}) as Session;
        renamed.title = title.trim();
        this.publish(id, { type: "renamed", title: renamed.title }, null);
        return renamed;
    }

    /** The sessions that were closed, by the id of their task, the one closed last first. */
    private readonly closedSessions = new Map<number, Session[]>();

    close(id: number) {
        this.interrupt(id);
        for (const task of this.tasks()) {
            const closing = task.sessions.filter((candidate) => candidate.id === id);
            task.sessions = task.sessions.filter((candidate) => candidate.id !== id);
            this.closedSessions.set(task.id, [...closing, ...this.closed(task.id)]);
            task.closed = this.closed(task.id).length;
        }
    }

    /** Gone for good, from the open ones and the closed ones alike. */
    delete(id: number) {
        this.interrupt(id);
        for (const task of this.tasks()) {
            const others = (sessions: Session[]) =>
                sessions.filter((candidate) => candidate.id !== id);
            task.sessions = others(task.sessions);
            this.closedSessions.set(task.id, others(this.closed(task.id)));
            task.closed = this.closed(task.id).length;
        }
    }

    closed(taskId: number): Session[] {
        return this.closedSessions.get(taskId) ?? [];
    }

    reopen(id: number): Session {
        for (const task of this.tasks()) {
            const back = this.closed(task.id).find((candidate) => candidate.id === id);
            if (back) {
                this.closedSessions.set(
                    task.id,
                    this.closed(task.id).filter((candidate) => candidate.id !== id),
                );
                task.sessions = [...task.sessions, back].sort((a, b) => a.id - b.id);
                task.closed = this.closed(task.id).length;
                return back;
            }
        }
        throw new Error(`session ${id} not found`);
    }

    interrupt(id: number) {
        this.interrupts.get(id)?.();
        this.interrupts.delete(id);
    }
}

/** Closing sessions, listing the closed ones of a task, and reopening one. */
/** The sample sessions whose title holds the words: the sample conversations are not kept. */
function sampleSearch(projects: Project[], query: string): SearchHit[] {
    const wanted = query.trim().toLowerCase();
    return projects.flatMap((project) =>
        project.tasks.flatMap((task) =>
            task.sessions
                .filter((session) => wanted !== "" && session.title.toLowerCase().includes(wanted))
                .map((session) => ({
                    session_id: session.id,
                    title: session.title,
                    project: project.name,
                    task: task.name,
                    closed: false,
                    snippet: `…${session.title}…`,
                    by_user: true,
                    matches: 1,
                })),
        ),
    );
}

/** A new sample session holding what was said in session `id`, as the core's fork does. */
/** Where message number `number` stands among `events`; their end, if there is no such message. */
function eventOfMessage(events: AgentEvent[], number: number): number {
    const places = events.flatMap((event, at) => (event.type === "user_message" ? [at] : []));
    return places[number] ?? events.length;
}

function sampleFork(
    agent: MockAgent,
    tasks: Task[],
    id: number,
    from: number | null,
    onAgent?: string,
): Session {
    const home = tasks.find((task) => task.sessions.some((one) => one.id === id));
    const source = home?.sessions.find((one) => one.id === id);
    if (!(home && source)) {
        throw new Error(`there is no session ${id}`);
    }
    const fork = {
        ...agent.newSession(home.id),
        title: `${source.title} (fork)`,
        agent: onAgent ?? source.agent,
    };
    const history = agent.history(id);
    const said = history.slice(from === null ? 0 : eventOfMessage(history, from));
    const kept = said.filter((event) => ["user_message", "text"].includes(event.type));
    agent.events.set(fork.id, [...kept, { type: "forked", from: id }]);
    home.sessions.push(fork);
    return fork;
}

/** `actions` with the one called `name` moved `by` places, no further than either end. */
function movedAction(actions: Action[], name: string, by: number): Action[] {
    const from = actions.findIndex((action) => action.name === name);
    const one = actions[from];
    if (!one) {
        return actions;
    }
    const rest = actions.filter((action) => action !== one);
    const to = Math.min(rest.length, Math.max(0, from + by));
    return [...rest.slice(0, to), one, ...rest.slice(to)];
}

/** The user's own buttons, held in memory: some for every repository, one for the first. */
function actionsApi(): Pick<Api, "actions" | "actionSave" | "actionMove" | "actionRemove"> {
    const one = (name: string, icon: string, prompt: string, command: string): Action => ({
        name,
        icon,
        prompt,
        command,
        of_project: false,
    });
    let general: Action[] = [
        one("Review", "🔍", "Review the changes in this worktree for bugs.", ""),
        one("Tests", "🧪", "", "pnpm test"),
        one("E2E, then mark", "✅", "Run the e2e tests and mark this session green.", ""),
    ];
    const own = new Map<string, Action[]>([
        ["storefront", [{ ...one("Nix env", "❄️", "", "nix develop"), of_project: true }]],
    ]);
    const offered = (project: string | null) => {
        const mine = project === null ? [] : (own.get(project) ?? []);
        const taken = new Set(mine.map((action) => action.name));
        return [...general.filter((action) => !taken.has(action.name)), ...mine];
    };
    const without = (actions: Action[], name: string) =>
        actions.filter((action) => action.name !== name);
    return {
        actions: async (project) => offered(project),
        actionSave: async (project, action) => {
            if (project === null) {
                general = [...without(general, action.name), action];
            } else {
                own.set(project, [...without(own.get(project) ?? [], action.name), action]);
            }
            return offered(project);
        },
        actionMove: async (project, name, by) => {
            if (project === null) {
                general = movedAction(general, name, by);
            } else {
                own.set(project, movedAction(own.get(project) ?? [], name, by));
            }
            return offered(project);
        },
        actionRemove: async (project, name) => {
            if (project === null) {
                general = without(general, name);
            } else {
                own.set(project, without(own.get(project) ?? [], name));
            }
            return offered(project);
        },
    };
}

/** Kept prompts, held in memory, starting with two samples. */
function templatesApi(): Pick<Api, "templates" | "templateSave" | "templateRemove"> {
    let kept: Template[] = [
        { name: "explain", text: "Explain how {{what}} works, briefly, with the files involved." },
        {
            name: "review",
            text: "Review the changes in this worktree for bugs. Do not change anything.",
        },
    ];
    const sorted = () => [...kept].sort((a, b) => a.name.localeCompare(b.name));
    return {
        templates: async () => sorted(),
        templateSave: async (name, text) => {
            kept = [...kept.filter((one) => one.name !== name), { name, text }];
            return sorted();
        },
        templateRemove: async (name) => {
            kept = kept.filter((one) => one.name !== name);
            return sorted();
        },
    };
}

const SAMPLE_CLOUD_DIFF = [
    "diff --git a/docs/api.md b/docs/api.md",
    "--- a/docs/api.md",
    "+++ b/docs/api.md",
    "@@ -10,3 +10,4 @@ ## Invoices",
    " `GET /invoices` lists them.",
    "-`POST /invoices` is not documented yet.",
    "+`POST /invoices` creates one from a JSON body.",
    "+`GET /invoices/:id/export` returns it as CSV.",
    " ",
].join("\n");

const SAMPLE_COSTS: Costs = {
    today: 3.42,
    week: 18.9,
    total: 212.05,
    tasks: [
        { project: "storefront", task: "fix-login-redirect", usd: 41.3 },
        { project: "bonsai-lint", task: "rust-support", usd: 12.75 },
        { project: "storefront", task: "invoice-export", usd: 0.4 },
    ],
    days: [
        { day: "2026-11-20", usd: 3.42 },
        { day: "2026-11-19", usd: 9.1 },
        { day: "2026-11-17", usd: 6.38 },
    ],
};

function closingApi(
    agent: MockAgent,
): Pick<Api, "sessionClose" | "sessionDelete" | "sessionsClosed" | "sessionReopen"> {
    const taskNamed = (name: string) =>
        agent.projects.flatMap((project) => project.tasks).find((task) => task.name === name);
    return {
        sessionClose: async (id) => agent.close(id),
        sessionDelete: async (id) => agent.delete(id),
        sessionsClosed: async (_project, name) => agent.closed(taskNamed(name)?.id ?? -1),
        sessionReopen: async (id) => agent.reopen(id),
    };
}

/** An in-memory stand-in for the app. */
/** What the sample agents' default efforts come to; the small model says nothing of effort. */
async function sampleEffortDefault(agent: string, model: string | null): Promise<string | null> {
    const known: Record<string, string> = { claude: "xhigh", codex: "medium" };
    return model === "haiku" ? null : (known[agent] ?? null);
}

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
    let customLayout: CustomLayout = { base: "classic" };
    let terminalConfig: TerminalConfig = {};
    let settings: Settings = {
        editor: "code",
        terminal_app: "Terminal",
        auto_title: true,
        theme: "system",
        palette: "swarm",
        style: "swarm",
        layout: "classic",
        text_scale: 100,
        default_supervision: "auto",
        html_views: true,
        notifications: true,
        rules: "",
        status_items: ["branch", "changes", "model", "context", "cost", "duration"],
        status_command: "",
    };
    return {
        overview: async () => structuredClone(agent.projects),
        ...mcpApi(),
        ...templatesApi(),
        ...actionsApi(),
        usage: async () => SAMPLE_USAGE,
        costs: async () => SAMPLE_COSTS,
        ...snapshotApi(() => agent.projects),
        analytics: async () => sampleAnalytics(new Date()),
        effortDefault: sampleEffortDefault,
        ...setupApi(hookListeners),
        agents: async () => [
            { id: "claude", name: "Claude Code", models: ["opus", "sonnet", "haiku", "fable"] },
            {
                id: "opencode",
                name: "OpenCode",
                models: ["openrouter/openai/gpt-5.6", "openrouter/google/gemini-3-pro"],
            },
        ],
        // A browser gives no paths, so the sample hands out two that look real.
        pickFiles: async () => [
            "/Users/you/Desktop/login-error.png",
            "/Users/you/code/storefront/README.md",
        ],
        onFilesDropped: async () => () => undefined,
        pickFolder: async () => "/Users/you/code/new-repo",
        projectAdd: async (path) => {
            const name = path.split("/").pop() ?? "repo";
            if (agent.projects.some((project) => project.name === name)) {
                throw new Error(`project \`${name}\` already exists`);
            }
            const project = {
                id: agent.projects.length + 1,
                name,
                repo_path: path,
                tasks: [],
                has_note: false,
                cloud: [],
            };
            agent.projects.push(project);
            return project;
        },
        ...removeApi(() => agent.projects),
        branchChoices: async (projectName) => ({
            base: "origin/main",
            existing:
                projectName === "storefront" ? ["feature/vat-export", "shop-118-invoices"] : [],
        }),
        // The sample data has no model to ask, so tasks are named after their prompts.
        taskNameFor: async () => null,
        ...environmentApi(),
        ...skillApi(),
        ...stackApi(() => agent.projects),
        ...cloudApi(() => agent.projects),
        taskNew: async (projectName, name, branch, environment) => {
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
                environment,
            };
            project.tasks.push(created);
            return created;
        },
        taskUnpublished: sampleUnpublished,
        taskCommitMessage: async () => {
            await new Promise((done) => setTimeout(done, 900));
            return "feat: show how long each turn took\n\nThe transcript only said that a turn had ended.";
        },
        taskPullMessage: async () => {
            await new Promise((done) => setTimeout(done, 900));
            return {
                title: "fix: stop the redirect loop after login",
                body: "**Logging in no longer loops back to the login page.**\n\n## Problem\n\n`next` could point at `/login` itself.\n\n## Change\n\n- Reject a `next` that leads back to the login page.\n- Fall back to the dashboard.",
            };
        },
        taskCommit: async () => undefined,
        taskPush: async () => undefined,
        taskPullDraft: async () => SAMPLE_DRAFT,
        taskPullCreate: sampleOpenedPull,
        taskPull: samplePull,
        taskPullOpen: async () => undefined,
        taskChanges: sampleChanges,
        taskDiff: sampleDiff,
        taskOpenEditor: async () => undefined,
        taskOpenTerminal: async () => undefined,
        taskFiles: async (_project, _name, dir) => sampleFiles(dir),
        taskOpenFile: async () => undefined,
        taskOpenUrl: async () => undefined,
        openLink: async (url) => {
            window.open(url, "_blank", "noopener");
        },
        ...noteApi(() => agent.projects),
        apiKeys: async () => structuredClone(keys),
        setApiKey: async (name, key) => storeKey(keys, name, key.trim() !== ""),
        deleteApiKey: async (name) => storeKey(keys, name, false),
        settings: async () => ({ ...settings }),
        // The sample's own: a browser has no app to ask.
        version: async () => "0.1.0",
        setBadge: async () => {},
        setSettings: async (changed) => {
            settings = changed;
        },
        terminalConfig: async () => ({ ...terminalConfig }),
        terminalConfigSave: async (text) => {
            terminalConfig = JSON.parse(text) as TerminalConfig;
            return { ...terminalConfig };
        },
        customLayout: async () => ({ ...customLayout }),
        // The real check is the core's; here only what is not JSON at all is refused.
        customLayoutSave: async (text) => {
            customLayout = { base: "classic", ...(JSON.parse(text) as Partial<CustomLayout>) };
            return { ...customLayout };
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
        sessionFork: async (id, from, onAgent) => sampleFork(agent, tasks(), id, from, onAgent),
        sessionEvents: async (id) => [...agent.history(id)],
        sessionSend: async (id, text, files) => agent.send(id, withFiles(text, files ?? [])),
        sessionQueue: async (id) => agent.queue(id),
        sessionEnqueue: async (id, text, files) => agent.enqueue(id, withFiles(text, files ?? [])),
        sessionQueueEdit: async (id, message, text) => agent.editQueued(id, message, text),
        sessionSendQueued: async (id) => agent.sendQueued(id),
        sessionMarkRead: async (id) => {
            agent.configure(id, { unread: false });
        },
        sessionAllow: async (id, requestId) => agent.decide(id, requestId, true),
        sessionAnswer: async (id, requestId, answers) => agent.decide(id, requestId, true, answers),
        configAnswer: async (requestId, accepted) => agent.answerProposal(requestId, accepted),
        sessionDeny: async (id, requestId) => agent.decide(id, requestId, false),
        sessionInterrupt: async (id) => agent.interrupt(id),
        ...backgroundApi(),
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
        sessionSetAgent: async (id, to) =>
            agent.configure(id, { agent: to, model: null, fast: false }),
        // A browser has no custom scheme; a blob address is enough to see the view.
        viewUrl: async (document) =>
            URL.createObjectURL(new Blob([document], { type: "text/html" })),
        viewOpen: async (document) => {
            window.open(URL.createObjectURL(new Blob([document], { type: "text/html" })));
        },
        terminalOpen: async (id) => terminal.open(id),
        terminalWrite: async (id, data) => terminal.type(id, data),
        terminalResize: async () => undefined,
        onTerminalOutput: async (listener) => {
            terminal.listeners.add(listener);
            return () => terminal.listeners.delete(listener);
        },
        statusLine: async (id) => scriptedStatus(settings.status_command, id),
        sessionRename: async (id, title) => agent.rename(id, title),
        sessionMark: async (id, colour, label) => agent.mark(id, colour, label),
        sessionsSearch: async (query) => sampleSearch(agent.projects, query),
        ...closingApi(agent),
        onSessionEvent: async (listener) => {
            agent.listeners.add(listener);
            return () => agent.listeners.delete(listener);
        },
        onNoticeOpened: async () => () => undefined,
        onEventsLagged: async () => () => undefined,
        // A browser has no menu bar.
        onMenu: async () => () => undefined,
    };
}

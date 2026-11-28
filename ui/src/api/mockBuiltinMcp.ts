import type { Api } from "./client";
import type {
    BuiltinMcpView,
    BuiltinRegistration,
    DoctorCheck,
    ExistingSetup,
    HeldSession,
    HookOutput,
    OldServer,
} from "./types";

// repolore as Swarmed's built-in MCP server in the sample (`pnpm dev`), as the core keeps it
// (`builtin_mcp.rs`). Settings → MCP servers walks it through every state once:
// 1. It starts not installed. Install prints a few lines, then it is off, with a setup of the
//    user's own found.
// 2. The first Copy is refused while a refresh runs; the second copies. Start fresh instead
//    turns on with nothing chosen to read, opening its page: the row then offers Open repolore
//    too, and the next check after the page was open takes the repositories as picked there.
// 3. Then it waits: on Claude Code's and Codex's registrations, which Remove takes out, on
//    OpenCode's, a step for the user, taken as done by the next Check again, and on the old
//    server, running with 2 sessions connected: an idle one of Swarmed's and one outside it.
//    Claude Code's settings hold a refresh hook.
// 4. Restart idle ones lets go of the old server. The next Check again finds it stopped, but
//    still in use by the session outside Swarmed, which would start it again; the one after
//    finds that session ended. With nothing left that gives on, with one check warning; the
//    old setup can then go to the Trash.
// 5. Turn off gives off, with the copy kept for turning on again.

type BuiltinApi = Pick<
    Api,
    | "builtinMcpServers"
    | "builtinMcpCheck"
    | "builtinMcpInstall"
    | "builtinMcpTurnOn"
    | "builtinMcpTurnOff"
    | "builtinMcpRemoveRegistration"
    | "builtinMcpRestartSessions"
    | "builtinMcpOpen"
    | "builtinMcpTrashOld"
>;

const NAME = "repolore";
/** How long a reading, a check and each printed line take, so that what runs shows. */
const PACE = 700;

/** Where the user's own repolore keeps its configuration and its index. */
const OLD_FOLDERS = ["/Users/you/.config/repolore", "/Users/you/.local/share/repolore"];

const REGISTRATIONS: BuiltinRegistration[] = [
    {
        key: "claude:user",
        client: "Claude Code",
        scope: "user",
        command: "claude mcp remove repolore --scope user",
        runs_here: true,
        step: null,
    },
    {
        key: "codex:user",
        client: "Codex",
        scope: "user",
        command: "codex mcp remove repolore",
        runs_here: true,
        step: null,
    },
    {
        key: "opencode:user",
        client: "OpenCode",
        scope: "user",
        command: null,
        runs_here: false,
        step: "OpenCode has no command for it: take the repolore entry out of ~/.config/opencode/opencode.json.",
    },
];

/** What each removal prints, as the agent's own command does. */
const REMOVED: Record<string, string[]> = {
    "claude:user": [
        'Removed MCP server "repolore" from user config',
        "File modified: ~/.claude.json",
    ],
    "codex:user": ["Removed global MCP server 'repolore'."],
};

const INSTALLED = [
    "Resolved 61 packages in 1.42s",
    "Downloading torch (72.4MiB)",
    "Prepared 61 packages in 6.18s",
    "Installed 61 packages in 214ms",
    "Installed 2 executables: repolore, repolore-mcp",
];

const OFFLINE_CHECKS: DoctorCheck[] = [
    { name: "Configuration", ok: true, warning: false, detail: "4 repositories", fix: null },
    {
        name: "Index",
        ok: true,
        warning: true,
        detail: "2 of 4 repositories were last refreshed 3 days ago",
        fix: "repolore refresh",
    },
    { name: "Embedding model", ok: true, warning: false, detail: "downloaded, 1.3 GB", fix: null },
    { name: "Registrations", ok: true, warning: false, detail: "only Swarmed's", fix: null },
];

const FULL_CHECKS: DoctorCheck[] = [
    ...OFFLINE_CHECKS,
    { name: "GitHub", ok: true, warning: false, detail: "every repository reachable", fix: null },
    { name: "Connection", ok: true, warning: false, detail: "answered in 0.4 s", fix: null },
];

function wait(ms: number) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

/** What a doctor and Claude Code say of the built-in: only once it is on. */
function health(on: boolean, checked: "offline" | "full" | null) {
    if (!on) {
        return { checks: [], checked: null, status: null };
    }
    const checks = checked === "full" ? FULL_CHECKS : OFFLINE_CHECKS;
    return { checks: [...checks], checked, status: "connected" };
}

/** repolore's walk through its states, printing as a hook does under `builtin:repolore`. */
class MockBuiltin {
    private installed = false;
    private turnedOn = false;
    private homeReady = false;
    private existing: ExistingSetup | null = { folders: OLD_FOLDERS, indexed: true };
    private registrations = [...REGISTRATIONS];
    /** Held by the idle session below and by one outside Swarmed. */
    private oldServer: OldServer | null = { running: true, pid: 48213, relays: 2 };
    private sessions: HeldSession[] = [
        { id: 2, title: "Write a regression test for the loop", idle: true },
    ];
    private copies = 0;
    private checked: "offline" | "full" | null = null;
    private pageOpen = false;

    constructor(private readonly hookListeners: Set<(output: HookOutput) => void>) {}

    private async print(lines: string[]) {
        for (const line of lines) {
            for (const listener of this.hookListeners) {
                listener({ project: "", task: `builtin:${NAME}`, line });
            }
            await wait(PACE);
        }
    }

    private state(): BuiltinMcpView["state"] {
        if (!this.installed) {
            return "missing";
        }
        if (!this.turnedOn) {
            return "off";
        }
        return this.registrations.length > 0 || this.oldServer ? "waiting" : "on";
    }

    view(): BuiltinMcpView {
        const state = this.state();
        return {
            name: NAME,
            state,
            version: this.installed ? "0.5.1" : null,
            minimum: "0.5.1",
            install_command:
                "uv tool install --managed-python --python 3.12 --exclude-newer-package repolore-mcp=false --upgrade 'repolore-mcp>=0.5.1'",
            home: "/Users/you/Library/Application Support/Swarmed/repolore",
            home_ready: this.homeReady,
            existing: this.existing && { ...this.existing },
            registrations: this.installed ? this.registrations.map((each) => ({ ...each })) : [],
            old_server: this.oldServer && { ...this.oldServer },
            sessions: this.sessions.map((each) => ({ ...each })),
            hook: true,
            ...health(state === "on", this.checked),
            page_open: this.pageOpen,
            problem: null,
        };
    }

    async read(): Promise<BuiltinMcpView> {
        await wait(PACE);
        return this.view();
    }

    /** A doctor run again: the user's own steps are taken as done, and a let-go server stopped. */
    async check(full: boolean): Promise<BuiltinMcpView> {
        await wait(full ? PACE * 3 : PACE * 2);
        this.registrations = this.registrations.filter((each) => each.runs_here);
        this.oldServer = this.oldServerChecked();
        // Once its page was open, the sample takes the repositories as picked there.
        this.homeReady ||= this.pageOpen;
        this.checked = full ? "full" : "offline";
        return this.view();
    }

    /**
     * The old server once Swarmed's own sessions let go of it: the first check finds it
     * stopped, as `repolore server stop` leaves it, with the session outside Swarmed still
     * connected; that one has ended by the next.
     */
    private oldServerChecked(): OldServer | null {
        if (this.oldServer === null || this.sessions.length > 0) {
            return this.oldServer;
        }
        if (!this.oldServer.running) {
            return null;
        }
        return { running: false, pid: null, relays: this.oldServer.relays };
    }

    async install(): Promise<BuiltinMcpView> {
        await this.print(INSTALLED);
        this.installed = true;
        return this.view();
    }

    /** Turns on; the first copy is refused, as repolore refuses one while a refresh runs. */
    async turnOn(copy: boolean): Promise<BuiltinMcpView> {
        await wait(PACE);
        if (copy && !this.homeReady) {
            this.copies += 1;
            if (this.copies === 1) {
                throw new Error("a refresh is running; try again when it is done");
            }
            this.homeReady = true;
        }
        // A fresh start's folder holds no setup until the repositories are picked in its page.
        this.turnedOn = true;
        this.checked = "offline";
        return this.view();
    }

    async turnOff(): Promise<BuiltinMcpView> {
        await wait(PACE);
        this.turnedOn = false;
        this.pageOpen = false;
        return this.view();
    }

    async remove(key: string): Promise<BuiltinMcpView> {
        const found = this.registrations.find((each) => each.key === key);
        if (!found?.runs_here) {
            throw new Error(`no registration ${key} that Swarmed can remove`);
        }
        await this.print(REMOVED[key] ?? []);
        this.registrations = this.registrations.filter((each) => each.key !== key);
        return this.view();
    }

    /** Restarts the idle sessions' agents, each of which lets go of its relay. */
    async restart(): Promise<BuiltinMcpView> {
        await wait(PACE);
        const restarted = this.sessions.filter((each) => each.idle).length;
        this.sessions = this.sessions.filter((each) => !each.idle);
        if (this.oldServer) {
            this.oldServer.relays = Math.max(0, this.oldServer.relays - restarted);
        }
        return this.view();
    }

    async open(): Promise<void> {
        if (!this.turnedOn) {
            throw new Error("repolore is off: turn it on to open its page");
        }
        await wait(PACE);
        this.pageOpen = true;
    }

    async trash(): Promise<BuiltinMcpView> {
        if (this.state() !== "on" || this.existing === null) {
            throw new Error("the old setup goes to the Trash only once repolore is on");
        }
        await wait(PACE);
        this.existing = null;
        return this.view();
    }
}

/** The built-in servers' part of the sample `Api`: repolore alone, walking its states. */
export function builtinMcpApi(hookListeners: Set<(output: HookOutput) => void>): BuiltinApi {
    const sample = new MockBuiltin(hookListeners);
    return {
        builtinMcpServers: async () => [await sample.read()],
        builtinMcpCheck: (_name, full) => sample.check(full),
        builtinMcpInstall: () => sample.install(),
        builtinMcpTurnOn: (_name, copy) => sample.turnOn(copy),
        builtinMcpTurnOff: () => sample.turnOff(),
        builtinMcpRemoveRegistration: (_name, key) => sample.remove(key),
        builtinMcpRestartSessions: () => sample.restart(),
        builtinMcpOpen: () => sample.open(),
        builtinMcpTrashOld: () => sample.trash(),
    };
}

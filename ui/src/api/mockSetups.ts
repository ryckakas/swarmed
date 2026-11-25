import { CHATS } from "../chats";
import { withFiles } from "../transcript";
import { titleFrom } from "../tree";
import type { Api } from "./client";
import type {
    AgentEvent,
    EnvironmentChoice,
    Project,
    Session,
    SetupEnd,
    SetupLog,
    SetupStep,
    SetupStop,
    Task,
} from "./types";

// How the sample repositories' tasks are set up, to see in a browser what a slow or a failing
// setup looks like: storefront's is quick, bonsai-lint's takes half a minute, and fsum's is
// killed the first time, as one the system ends for memory is, and does well when run again.

interface Script {
    /** What it prints, one line at a time. */
    lines: string[];
    /** How long each line takes, in milliseconds. */
    pace: number;
    /** Why it fails, if it does. */
    fails: string | null;
}

/** What the core says of a hook killed from outside (`failure_said`). */
export const KILLED =
    "`setup` was killed (signal 9, SIGKILL) by something outside it: the system, which ends programs like this when memory runs low, or another program, such as another Swarmed removing the task. This Swarmed did not end it.";

/** What the core says of each way a setup is stopped (`SetupStop::reason`). */
const STOPPED: Record<SetupStop, string> = {
    removed: "Stopped because the task was removed.",
    asked: "Stopped because Swarmed quit, or the command line that ran it was interrupted.",
    user: "Stopped by you.",
};

const INSTALL = ["Resolving packages", "Downloading 1,284 packages", "Linking dependencies"];

/** How repository `project`'s setup goes, the `tries`th time for one task. */
function scriptFor(project: string, tries: number): Script {
    if (project === "bonsai-lint") {
        const crates = Array.from(
            { length: 36 },
            (_, at) => `   Compiling crate-${at + 1} v0.${at}.0`,
        );
        return {
            lines: [...INSTALL, ...crates, "    Finished dev profile"],
            pace: 750,
            fails: null,
        };
    }
    if (project === "fsum" && tries === 0) {
        const lines = [...INSTALL, "Building native modules", "node-gyp rebuild"];
        return { lines, pace: 700, fails: KILLED };
    }
    return { lines: [...INSTALL, "Seeding the database"], pace: 450, fails: null };
}

/** The bytes `text` takes, as the core counts places in a log. */
function bytes(text: string): number {
    return new TextEncoder().encode(text).length;
}

function pause(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

interface Run {
    began: number;
    step: SetupStep;
    stopped: SetupStop | null;
}

/** The sample's setups, told as the core tells them: `setup` events with session id 0. */
export class MockSetups {
    private readonly logs = new Map<number, string>();
    private readonly running = new Map<number, Run>();
    private readonly tries = new Map<number, number>();

    constructor(private readonly tell: (event: AgentEvent) => void) {}

    /** Whether the setup of task `id` is under way. */
    runs(id: number): boolean {
        return this.running.has(id);
    }

    /**
     * Sets `task` of repository `project` up, keeping its `setup`, `setup_reason` and
     * `setting_up` as the core's tree would have them. Resolves to how it ended.
     */
    async run(project: string, task: Task): Promise<SetupEnd> {
        const tries = this.tries.get(task.id) ?? 0;
        this.tries.set(task.id, tries + 1);
        const script = scriptFor(project, tries);
        const run: Run = { began: Date.now(), step: "preparing", stopped: null };
        this.running.set(task.id, run);
        this.logs.set(task.id, "");
        task.setting_up = { state: "here", step: run.step, began: run.began, latest: null };
        await pause(400);
        this.step(task, run, "hook");
        for (const line of script.lines) {
            if (run.stopped !== null) {
                break;
            }
            this.say(task, run, line);
            await pause(script.pace);
        }
        const end = await this.ending(task, run, script);
        this.running.delete(task.id);
        return end;
    }

    /** Stops the setup of task `id`, if one runs, for `why`. */
    stop(id: number, why: SetupStop) {
        const run = this.running.get(id);
        if (run) {
            run.stopped = why;
        }
    }

    /** The end of what the last setup of task `id` printed. */
    log(id: number): SetupLog {
        const text = this.logs.get(id) ?? "";
        return { text, end: bytes(text) };
    }

    /** How a run ends: stopped, failed, or with its stack started; said last. */
    private async ending(task: Task, run: Run, script: Script): Promise<SetupEnd> {
        let end: SetupEnd;
        if (run.stopped !== null) {
            end = { outcome: "stopped", why: run.stopped, reason: STOPPED[run.stopped] };
        } else if (script.fails !== null) {
            end = { outcome: "failed", reason: script.fails };
        } else {
            this.step(task, run, "stack");
            await pause(400);
            end = { outcome: "done" };
        }
        const took = ((Date.now() - run.began) / 1000).toFixed(1);
        const reason = end.outcome === "done" ? null : end.reason;
        const last =
            reason === null
                ? `setup finished in ${took} s`
                : `setup ended after ${took} s: ${reason}`;
        // Written down before the last report, as the core does: the tree read again on it
        // has to say how it ended.
        const said = `${last}\n`;
        task.setting_up = null;
        task.setup = end.outcome === "done" ? "ok" : "failed";
        task.setup_reason = reason;
        this.report(task, run, said, end);
        return end;
    }

    private step(task: Task, run: Run, step: SetupStep) {
        run.step = step;
        if (task.setting_up?.state === "here") {
            task.setting_up.step = step;
        }
        this.report(task, run, "", null);
    }

    private say(task: Task, run: Run, line: string) {
        if (task.setting_up?.state === "here") {
            task.setting_up.latest = line;
        }
        this.report(task, run, `${line}\n`, null);
    }

    private report(task: Task, run: Run, output: string, ended: SetupEnd | null) {
        const before = this.logs.get(task.id) ?? "";
        this.logs.set(task.id, before + output);
        const from = bytes(before);
        this.tell({
            type: "setup",
            progress: {
                task_id: task.id,
                step: run.step,
                began: run.began,
                output,
                from,
                to: from + bytes(output),
                ended,
            },
        });
    }
}

/** What the sample's tasks and sessions need of the rest of the mock. */
export interface SetupDeps {
    projects(): Project[];
    /** Makes a task as the core writes one down, before its setup, saying what it does. */
    make(
        project: string,
        name: string,
        branch: string | null,
        choice: EnvironmentChoice,
    ): Promise<Task>;
    /** A new session in task `taskId`, which nothing has been said to. */
    newSession(taskId: number): Session;
    /** Keeps images pasted into a first message as session `id`'s own; their paths. */
    keepImages(id: number, images: string[]): Promise<string[]>;
    enqueue(id: number, text: string): void;
    /** Sends the session's first waiting message, as the end of a setup or a turn does. */
    sendQueued(id: number): void;
    send(id: number, text: string): void;
}

/** The repository and task called so, if there is such a task. */
function found(projects: Project[], project: string, name: string) {
    const home = projects.find((candidate) => candidate.name === project);
    const task = home?.tasks.find((candidate) => candidate.name === name);
    if (!(home && task)) {
        throw new Error(`task \`${name}\` not found`);
    }
    return { home, task };
}

/** Starting tasks and setting them up, as the core does it, on the sample's own setups. */
export function taskSetupApi(
    setups: MockSetups,
    deps: SetupDeps,
): Pick<
    Api,
    "taskNew" | "taskStart" | "taskSetup" | "taskSetupStop" | "taskStartAnyway" | "taskSetupLog"
> {
    /** Sends what the task's sessions waited to send until it was set up. */
    const sendWaiting = (task: Task) => {
        for (const session of task.sessions.filter((session) => session.waits_for_setup)) {
            session.waits_for_setup = false;
            deps.sendQueued(session.id);
        }
    };
    /** Sets the task up and, once it finished well, sends what waited for it. */
    const setUp = async (project: string, task: Task): Promise<Task> => {
        const end = await setups.run(project, task);
        if (end.outcome !== "done") {
            throw new Error(end.reason);
        }
        sendWaiting(task);
        return task;
    };
    return {
        taskNew: async (project, name, branch, choice) => {
            const task = await deps.make(project, name, branch, choice);
            return project === CHATS ? task : setUp(project, task);
        },
        taskStart: async (project, name, start) => {
            const task = await deps.make(project, name, start.branch, start.environment);
            // A chat has no repository, and so no issue.
            task.issue = project === CHATS ? null : start.issue;
            const session: Session = {
                ...deps.newSession(task.id),
                ...start.setup,
                agent: start.setup.agent ?? "claude",
                fast: start.setup.fast ?? false,
                title: start.title ?? titleFrom(start.prompt),
                waits_for_setup: project !== CHATS,
            };
            const kept = await deps.keepImages(session.id, start.images);
            const prompt = withFiles(start.prompt, [...start.files, ...kept]);
            task.sessions.push(session);
            if (project === CHATS) {
                deps.send(session.id, prompt);
            } else {
                deps.enqueue(session.id, prompt);
                // Goes on by itself; how it ends is told, and kept in the tree.
                setUp(project, task).catch(() => undefined);
            }
            return { task, session };
        },
        taskSetup: async (project, name) => {
            const { task } = found(deps.projects(), project, name);
            if (setups.runs(task.id)) {
                throw new Error(`the setup of \`${name}\` is running already`);
            }
            return setUp(project, task);
        },
        taskSetupStop: async (project, name) => {
            setups.stop(found(deps.projects(), project, name).task.id, "user");
        },
        taskStartAnyway: async (project, name) => {
            const { task } = found(deps.projects(), project, name);
            const waiting = task.sessions.filter((session) => session.waits_for_setup);
            if (waiting.length === 0) {
                throw new Error(`nothing of \`${name}\`'s sessions waits for its setup`);
            }
            sendWaiting(task);
            return waiting.map((session) => session.id);
        },
        taskSetupLog: async (project, name) =>
            setups.log(found(deps.projects(), project, name).task.id),
    };
}

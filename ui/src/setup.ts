import type {
    SessionStatus,
    SettingUp,
    SetupEnd,
    SetupLog,
    SetupProgress,
    SetupStep,
    Task,
} from "./api/types";

// How a task's setup is followed and told: what the window keeps of what it printed, put
// together from its log and from what the core publishes as it goes, and the words for it.
// The core counts places in the log in bytes; text here is JavaScript's, so the two are
// matched by encoding.

/** What the window keeps of one task's setup. */
export interface SetupSeen {
    /** When the run began, which tells it from the next; `null` for one only read from its log. */
    began: number | null;
    /** Where it was when last heard of; `null` for one only read from its log. */
    step: SetupStep | null;
    /** The end of what it printed. */
    text: string;
    /** Where in the log `text` starts and ends, in bytes. */
    start: number;
    end: number;
    /** How it ended, once that was heard. */
    ended: SetupEnd | null;
}

/** How much of what a setup printed is kept: the end of it, from the start of a line. */
export const KEPT_BYTES = 64 * 1024;

const encoder = new TextEncoder();
const decoder = new TextDecoder();

function bytesOf(text: string): number {
    return encoder.encode(text).length;
}

/** `text` without its first `count` bytes. */
function afterBytes(text: string, count: number): string {
    return count <= 0 ? text : decoder.decode(encoder.encode(text).slice(count));
}

/** `seen` holding no more than [`KEPT_BYTES`], the end of it, from the start of a line. */
function trimmed(seen: SetupSeen): SetupSeen {
    const bytes = encoder.encode(seen.text);
    if (bytes.length <= KEPT_BYTES) {
        return seen;
    }
    const cut = bytes.length - KEPT_BYTES;
    const newline = bytes.indexOf(10, cut);
    const from = newline === -1 ? cut : newline + 1;
    return { ...seen, text: decoder.decode(bytes.slice(from)), start: seen.start + from };
}

/** `seen` with `more`, which starts at byte `from` of the log, added where it goes on. */
function goneOn(seen: SetupSeen, more: string, from: number): SetupSeen {
    if (from >= seen.end) {
        // Something heard of before this may have been missed: what follows is still worth having.
        return { ...seen, text: seen.text + more, end: from + bytesOf(more) };
    }
    const after = from + bytesOf(more);
    if (after <= seen.end) {
        return seen;
    }
    return { ...seen, text: seen.text + afterBytes(more, seen.end - from), end: after };
}

/** Whether a run of the setup was heard of since `before` was what the window kept of it. */
export function ranSince(before: SetupSeen | undefined, now: SetupSeen | undefined): boolean {
    return now !== undefined && now.began !== null && now.began !== before?.began;
}

/** What is kept of a setup once one of its reports is heard. A new run starts afresh. */
export function heard(seen: SetupSeen | undefined, progress: SetupProgress): SetupSeen {
    const fresh = seen === undefined || seen.began !== progress.began;
    const base: SetupSeen = fresh
        ? {
              began: progress.began,
              step: progress.step,
              text: "",
              start: progress.from,
              end: progress.from,
              ended: null,
          }
        : seen;
    const moved = { ...base, step: progress.step, ended: progress.ended ?? base.ended };
    return trimmed(goneOn(moved, progress.output, progress.from));
}

/**
 * What is kept of a setup once its log is read. What was heard of the same run meanwhile goes
 * on from where the log ends; anything else heard is older than the log, which takes its place.
 */
export function readFrom(
    seen: SetupSeen | undefined,
    log: SetupLog,
    began: number | null,
): SetupSeen {
    const fromLog: SetupSeen = {
        began,
        step: seen?.step ?? null,
        text: log.text,
        start: log.end - bytesOf(log.text),
        end: log.end,
        ended: seen?.ended ?? null,
    };
    if (seen === undefined || began === null || seen.began !== began) {
        return trimmed(fromLog);
    }
    return trimmed(
        goneOn(
            fromLog,
            afterBytes(seen.text, Math.max(0, log.end - seen.start)),
            Math.max(log.end, seen.start),
        ),
    );
}

/** The last `count` lines of `text`. */
export function lastLines(text: string, count: number): string {
    const lines = text.replace(/\n$/, "").split("\n");
    return lines.slice(-count).join("\n");
}

/** Where a setup stands, in words. */
export function stepSaid(step: SetupStep | null): string {
    switch (step) {
        case "preparing":
            return "Making the worktree ready";
        case "waiting":
            return "Waiting for another setup of this repository to finish (max_setups)";
        case "hook":
            return "Running setup";
        case "keeping":
            return "Keeping a snapshot for the next tasks";
        case "stack":
            return "Starting the stack";
        default:
            return "Setting up";
    }
}

/** How long something has run, from `began` to `now`, both in milliseconds: `45s`, `2m 05s`. */
export function elapsed(began: number, now: number): string {
    const seconds = Math.max(0, Math.floor((now - began) / 1000));
    if (seconds < 60) {
        return `${seconds}s`;
    }
    const minutes = Math.floor(seconds / 60);
    return `${minutes}m ${String(seconds % 60).padStart(2, "0")}s`;
}

/** Where a task's setup stands, as the window tells it. */
export type SetupState =
    /** Under way, here or by another Swarmed. */
    | "running"
    | "elsewhere"
    /** Over without finishing well, or cut short with nothing setting it up. */
    | "failed"
    | "interrupted"
    /** Over and well, or never needed. */
    | "done";

export function setupState(task: Pick<Task, "setup" | "setting_up">): SetupState {
    const now: SettingUp | null = task.setting_up ?? null;
    if (now?.state === "here") {
        return "running";
    }
    if (now?.state === "elsewhere" || now?.state === "interrupted") {
        return now.state;
    }
    return task.setup === "failed" ? "failed" : "done";
}

/**
 * How long a setup under way here runs before the window shows it, in milliseconds. One with
 * nothing to run ends sooner and is never seen: a panel that comes and goes again at once only
 * draws the eye and moves the conversation down and back. Shorter than `useNow`'s tick, so that
 * a longer setup shows at its first tick after this.
 */
export const SETUP_GRACE = 800;

/**
 * Whether the window shows a task's setup, in the session's panel and under the task in the
 * sidebar. One under way here shows once it has run [`SETUP_GRACE`] by `now`, counted from when
 * it began; a report of this run's end decides before the tree is read again: shown at once if
 * it failed or was stopped, never if it finished well or was stopped for its task's removal,
 * which the window says nothing of. Any other that is not done shows at once: failed, stopped
 * or cut short, or run by another Swarmed, which says nothing of when it began. Only what is
 * drawn waits: what is typed waits for the setup all the same (`standing`).
 */
export function setupShown(
    task: Pick<Task, "setup" | "setting_up">,
    seen: SetupSeen | undefined,
    now: number,
): boolean {
    const at = task.setting_up;
    if (at?.state !== "here") {
        return setupState(task) !== "done";
    }
    const ended = seen?.began === at.began ? seen.ended : null;
    if (ended !== null) {
        return (
            ended.outcome === "failed" || (ended.outcome === "stopped" && ended.why !== "removed")
        );
    }
    return now - at.began >= SETUP_GRACE;
}

/** Whether a task's setup is under way, here or elsewhere. */
export function isSettingUp(task: Pick<Task, "setup" | "setting_up">): boolean {
    const state = setupState(task);
    return state === "running" || state === "elsewhere";
}

/**
 * How a session stands for deciding whether a message waits its turn: one whose task is being
 * set up waits as a busy one does, so that what is typed goes after the setup and after the
 * message the session was made with.
 */
export function standing(
    status: SessionStatus | undefined,
    task: Pick<Task, "setup" | "setting_up"> | undefined,
): SessionStatus | undefined {
    return task && isSettingUp(task) ? "working" : status;
}

/** Why a setup is not done, in words: its own reason, or what cutting it short means. */
export function whyNotDone(task: Pick<Task, "setup" | "setting_up" | "setup_reason">): string {
    if (setupState(task) === "interrupted") {
        return "It never finished: the Swarmed that ran it quit or crashed first. Run it again, or start anyway.";
    }
    return task.setup_reason ?? "It did not finish.";
}

/**
 * Where a setup under way here is now: as its latest report said, if one of this run was
 * heard, since the tree is only read again when a run begins and ends; else as the tree has it.
 */
export function stepNow(
    task: Pick<Task, "setting_up">,
    seen: SetupSeen | undefined,
): SetupStep | null {
    const at = task.setting_up;
    if (at?.state !== "here") {
        return null;
    }
    return seen?.began === at.began && seen.step !== null ? seen.step : at.step;
}

/** The headline of a setup that is not done. */
export function headline(
    task: Pick<Task, "setup" | "setting_up" | "setup_reason">,
    seen?: SetupSeen,
): string {
    switch (setupState(task)) {
        case "running":
            return stepSaid(stepNow(task, seen));
        case "elsewhere":
            return "Being set up by another Swarmed";
        case "interrupted":
            return "Setup was interrupted";
        case "failed":
            return stopped(task) ? "Setup was stopped" : "Setup failed";
        default:
            return "Set up";
    }
}

/**
 * What the sidebar says under a task whose setup is not done: the last line it printed or how
 * long it has run, while it runs; a flag once it failed or was cut short. `null` for one that is
 * done.
 */
export function sidebarSays(
    task: Pick<Task, "setup" | "setting_up" | "setup_reason">,
    seen: SetupSeen | undefined,
    now: number,
): { busy: boolean; text: string } | null {
    const at = task.setting_up;
    switch (setupState(task)) {
        case "running": {
            const began = at?.state === "here" ? at.began : now;
            const said =
                lastLines(seen?.text ?? "", 1) || (at?.state === "here" ? at.latest : null);
            const waiting = stepNow(task, seen) === "waiting";
            const where = waiting ? "waiting for a place" : said || "starting";
            return { busy: true, text: `Setting up… ${elapsed(began, now)} · ${where}` };
        }
        case "elsewhere":
            return { busy: true, text: "Setting up in another Swarmed…" };
        case "interrupted":
            return { busy: false, text: "setup interrupted" };
        case "failed":
            return { busy: false, text: stopped(task) ? "setup stopped" : "setup failed" };
        default:
            return null;
    }
}

/** Whether a setup that did not finish was stopped, rather than failing by itself. */
function stopped(task: Pick<Task, "setup_reason">): boolean {
    return task.setup_reason?.startsWith("Stopped") ?? false;
}

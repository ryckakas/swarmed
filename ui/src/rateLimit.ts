// What Claude Code reports about the user's usage limits, in words for the transcript. It
// reports after every turn: `allowed`, `allowed_warning` while a window fills up, and another
// status once a limit is reached. A warning is not a limit reached, and is said once, when it
// starts, not again with every turn. Which warnings are said at all is the user's setting
// (`LimitWarnings`); a limit reached always is.

import type { AccountUsage, AgentEvent, LimitWarnings } from "./api/types";

/** One usage report of Claude Code's. */
export type RateLimit = Extract<AgentEvent, { type: "rate_limit" }>;

/** What Claude Code calls its usage windows, and what they are called here. */
const WINDOWS: Record<string, string> = {
    five_hour: "5-hour",
    seven_day: "weekly",
    seven_day_opus: "weekly Opus",
    seven_day_sonnet: "weekly Sonnet",
    seven_day_overage_included: "weekly, extra usage included",
};

/** What a window is called here, from Claude Code's name for it. */
function windowName(type: string): string {
    return WINDOWS[type] ?? type.replaceAll("_", " ");
}

function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null;
}

/** Whether a report says extra usage pays for the turn: the plan's limit is reached, and the work goes on, billed. */
function onExtraUsage(event: RateLimit): boolean {
    const raw = isRecord(event.raw) ? event.raw : {};
    return raw.isUsingOverage === true || raw.overageInUse === true;
}

/** Which window a report is about, and how much of it is used, where Claude Code says. */
function details(event: RateLimit): { window: string | null; used: number | null } {
    const raw = isRecord(event.raw) ? event.raw : {};
    const type = raw.rateLimitType;
    const window = typeof type === "string" ? windowName(type) : null;
    const used = typeof raw.utilization === "number" ? raw.utilization : null;
    return { window, used };
}

/**
 * When a window resets: the time alone if that is today, and the day as well if not, since a
 * weekly window can be days away.
 */
export function resetWhen(seconds: number, now: Date): string {
    const at = new Date(seconds * 1000);
    const time = at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    if (at.toDateString() === now.toDateString()) {
        return `at ${time}`;
    }
    const day = at.toLocaleDateString([], { weekday: "short", day: "numeric", month: "short" });
    return `on ${day} at ${time}`;
}

/**
 * What the transcript says about a usage report, or `null` for nothing: nothing while all is
 * well, a warning when one starts that `mode` says, and every time a limit is reached, whatever
 * `mode` is. `previous` is the report before it in the conversation.
 */
export function limitNotice(
    event: RateLimit,
    previous: RateLimit | null,
    now: Date,
    mode: LimitWarnings,
): string | null {
    if (event.status === "allowed") {
        return null;
    }
    const { window } = details(event);
    const resets = event.resets_at === null ? "" : ` It resets ${resetWhen(event.resets_at, now)}.`;
    if (onExtraUsage(event)) {
        return billedNotice(previous, window, resets);
    }
    if (event.status !== "allowed_warning") {
        return `Usage limit reached${window ? ` (${window})` : ""}.${resets}`;
    }
    return warningNotice(event, previous, mode, resets);
}

/**
 * Whether `mode` says a report's warning: every one when frequent; when rare, only one that says
 * its window is at least `LIMIT_WARNING` full, since Claude Code warns from about half; none
 * when none.
 */
function says(event: RateLimit, mode: LimitWarnings): boolean {
    if (event.status !== "allowed_warning" || mode === "none") {
        return false;
    }
    const { used } = details(event);
    return mode === "frequent" || (used !== null && used >= LIMIT_WARNING);
}

/**
 * A warning that a window fills up, said once: not while the report before it was a warning of
 * the same window that `mode` said too. A warning `mode` left unsaid was not news yet, so the
 * one that passes the mark is.
 */
function warningNotice(
    event: RateLimit,
    previous: RateLimit | null,
    mode: LimitWarnings,
    resets: string,
): string | null {
    const { window, used } = details(event);
    const saidAlready =
        previous !== null && says(previous, mode) && details(previous).window === window;
    if (!says(event, mode) || saidAlready) {
        return null;
    }
    const share = used === null ? "" : `: ${Math.round(used * 100)}% used`;
    return `Claude Code warns that the ${window ?? "usage"} limit is getting closer${share}. Nothing is limited yet.${resets}`;
}

/**
 * A limit reached while extra usage carries the work on: said once, not with every turn that
 * goes on billed, as long as the reports before it said the same of the same window.
 */
function billedNotice(
    previous: RateLimit | null,
    window: string | null,
    resets: string,
): string | null {
    const saidAlready =
        previous !== null && onExtraUsage(previous) && details(previous).window === window;
    if (saidAlready) {
        return null;
    }
    const which = window ? `The ${window} limit` : "A usage limit";
    return `${which} is reached: Claude Code goes on with extra usage, which is billed.${resets}`;
}

/** One of the account's usage windows, as it stands now. */
export interface LimitWindow {
    /** What the window is called here, such as "5-hour" or "weekly". */
    name: string;
    /** How much of it is used, from 0 to 1. */
    used: number;
    /** When it resets, in seconds since 1970; `null` if not said. */
    resetsAt: number | null;
    /** `allowed`, or what stops the next request. */
    status: string;
    /** When its figure was heard, in seconds since 1970. */
    heardAt: number;
    /** Whether its figure came from asking Claude Code, and not from a report after a turn. */
    asked: boolean;
    /** Whether it has reset since its figure was heard: it counts as empty until a report says otherwise. */
    reset: boolean;
}

/** Claude Code warns once a window is this full, and so does Swarmed. */
export const LIMIT_WARNING = 0.75;

/** How a window stands: well within, past the warning, or used up. */
export type LimitLevel = "fine" | "warning" | "reached";

/**
 * How a window stands, by how full it is. A report's own word for it is not taken: Claude Code
 * has said `allowed_warning` of a window less than a third full.
 */
export function limitLevel(window: LimitWindow): LimitLevel {
    if (window.used >= 1 || window.status === "rejected") {
        return "reached";
    }
    return window.used >= LIMIT_WARNING ? "warning" : "fine";
}

/**
 * The window most worth a warning: one used up before one past the warning, and of those the
 * fullest. `null` while all are fine.
 */
export function limitWarning(windows: LimitWindow[]): LimitWindow | null {
    const weight = (window: LimitWindow) =>
        (limitLevel(window) === "reached" ? 2 : 0) + window.used;
    const pressing = windows.filter((window) => limitLevel(window) !== "fine");
    return pressing.reduce<LimitWindow | null>(
        (worst, window) => (worst === null || weight(window) > weight(worst) ? window : worst),
        null,
    );
}

/** What a window past the warning comes to, in a sentence of its own. */
export function warningLine(window: LimitWindow): string {
    const name = window.name.charAt(0).toUpperCase() + window.name.slice(1);
    if (limitLevel(window) === "reached") {
        return `${name}: used up.`;
    }
    return `${name}: ${Math.round(window.used * 100)}% used.`;
}

/**
 * The account's usage windows as they stand at `now`, in seconds since 1970: each as the newest
 * report any Claude Code session heard gave it, except that one past its reset counts as reset,
 * at 0%, until a report says otherwise. In the core's order, the 5-hour window first. Only
 * Claude Code reports them: the Agent Client Protocol has no message for an account's limits.
 */
export function accountWindows(usage: AccountUsage | null, now: number): LimitWindow[] {
    return (usage?.windows ?? []).map((window) => {
        const reset = window.resets_at !== null && window.resets_at <= now;
        return {
            name: window.name,
            used: reset ? 0 : window.used,
            resetsAt: window.resets_at,
            status: reset ? "allowed" : window.status,
            heardAt: window.heard_at,
            asked: window.asked,
            reset,
        };
    });
}

/** The longest a timer is set for: browsers take a longer one for none at all. */
const LONGEST_WAIT_MS = 2 ** 31 - 1;

/** How long after a reset what is shown changes, so that the reset has surely passed by then. */
const AFTER_RESET_MS = 1000;

/**
 * How long until the next window, or the limit extra usage paid past, resets, in milliseconds
 * from `now` (seconds since 1970): the time to look again, with nothing asked of Claude Code.
 * `null` when nothing is to reset.
 */
export function untilNextReset(usage: AccountUsage | null, now: number): number | null {
    const times = [
        ...(usage?.windows ?? []).map((window) => window.resets_at),
        usage?.billed?.resets_at ?? null,
    ].filter((at): at is number => at !== null && at > now);
    if (times.length === 0) {
        return null;
    }
    return Math.min((Math.min(...times) - now) * 1000 + AFTER_RESET_MS, LONGEST_WAIT_MS);
}

/** How old a figure is, heard at `seconds`: the time alone if that is today, else the day too. */
export function asOf(seconds: number, now: Date): string {
    const at = new Date(seconds * 1000);
    const time = at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    if (at.toDateString() === now.toDateString()) {
        return `as of ${time}`;
    }
    const day = at.toLocaleDateString([], { weekday: "short", day: "numeric", month: "short" });
    return `as of ${day}, ${time}`;
}

/** A figure is older than the newest one heard if it was heard this long before it. */
const OLDER_BY = 60;

/** When the newest of `windows` was heard, in seconds since 1970; `null` for none. */
export function newestHeard(windows: LimitWindow[]): number | null {
    return windows.reduce<number | null>(
        (newest, window) => (newest === null || window.heardAt > newest ? window.heardAt : newest),
        null,
    );
}

/** Whether a window's figure is older than the newest heard, and says its own age. */
function heardBefore(window: LimitWindow, newest: number | null): boolean {
    return newest !== null && window.heardAt < newest - OLDER_BY;
}

/** What is said of a window beside its figure, each `null` for nothing to say. */
export interface WindowNotes {
    /** When it resets, or that it has reset since its figure was heard. */
    resets: string | null;
    /** How old its figure is, if that is older than the newest heard. */
    age: string | null;
}

/** What is said of a window, whose figures are older than the newest heard (`newest`). */
export function windowNotes(window: LimitWindow, newest: number | null, now: Date): WindowNotes {
    const when = window.resetsAt === null ? null : resetWhen(window.resetsAt, now);
    return {
        resets: when === null ? null : `${window.reset ? "Reset" : "Resets"} ${when}`,
        age: heardBefore(window, newest) ? `Heard ${resetWhen(window.heardAt, now)}` : null,
    };
}

/** A window's notes in one line, as the narrow context card shows them; `null` for none. */
export function windowNote(notes: WindowNotes): string | null {
    const parts = [notes.resets, notes.age].filter((part) => part !== null);
    return parts.length === 0 ? null : parts.join(" · ");
}

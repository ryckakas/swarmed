// What Claude Code reports about the user's usage limits, and what Swarmed makes of it. It
// reports after every turn, one window at a time: `allowed`, `allowed_warning` while a window
// fills up, and another status once a limit is reached. Reports of two windows come in turn,
// so nothing is said per report. The account's windows, as the newest reports of any session
// left them, make one usage notice at the top of the window, by the highest level reached
// (`usageNotice`); which levels it says is the user's setting (`LimitWarnings`), and dismissing
// it keeps it away until a higher level or a reset (`Dismissed`). The conversation says only
// that a limit stopped a turn (`refusalNotice`).

import type { AccountUsage, AgentEvent, LimitWarnings } from "./api/types";

/** One usage report of Claude Code's. */
export type RateLimit = Extract<AgentEvent, { type: "rate_limit" }>;

/** What Claude Code calls its usage windows, and what they are called here. */
const WINDOWS: Record<string, string> = {
    five_hour: "5-hour",
    seven_day: "weekly",
    seven_day_opus: "weekly Opus",
    seven_day_sonnet: "weekly Sonnet",
    // One model's own weekly limit, which a report after a turn does not name.
    seven_day_overage_included: "weekly model",
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

/** Which window a report is about, where Claude Code says. */
function windowOf(event: RateLimit): string | null {
    const raw = isRecord(event.raw) ? event.raw : {};
    return typeof raw.rateLimitType === "string" ? windowName(raw.rateLimitType) : null;
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
 * When a session goes on by itself, as the clock beside it says: the time alone if that is
 * today, and the day's short name before it if not.
 */
export function clockTime(seconds: number, now: Date): string {
    const at = new Date(seconds * 1000);
    const time = at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    if (at.toDateString() === now.toDateString()) {
        return time;
    }
    return `${at.toLocaleDateString([], { weekday: "short" })} ${time}`;
}

/**
 * What the conversation says about a usage report: that a limit was reached with nothing to
 * pay past it, so the turn was refused, or `null` for nothing. That explains why the agent
 * stopped where it did. A warning, and a limit extra usage pays past, are for the usage notice
 * at the top of the window alone.
 */
export function refusalNotice(event: RateLimit, now: Date): string | null {
    if (event.status === "allowed" || event.status === "allowed_warning" || onExtraUsage(event)) {
        return null;
    }
    const window = windowOf(event);
    const resets = event.resets_at === null ? "" : ` It resets ${resetWhen(event.resets_at, now)}.`;
    return `Usage limit reached${window ? ` (${window})` : ""}.${resets}`;
}

/** One of the account's usage windows, as it stands now. */
export interface LimitWindow {
    /** What tells it apart, in Claude Code's words, such as `five_hour` or `seven_day`. */
    kind: string;
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
 * has said `allowed_warning` of a window less than a third full. Reached is a report's word for
 * it, or a report after a turn at 100% or more: an answer to asking gives a whole percent, and
 * its 100 may be 99.5, as when a report said 99% at that very second.
 */
export function limitLevel(window: LimitWindow): LimitLevel {
    if (window.status === "rejected" || (!window.asked && window.used >= 1)) {
        return "reached";
    }
    return window.used >= LIMIT_WARNING ? "warning" : "fine";
}

/** Whether a window is one model's own weekly limit, which stops that model alone. */
export function oneModels(window: LimitWindow): boolean {
    return window.kind.startsWith("seven_day_");
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
 * How far the account's usage has come, one level for every window: 0 for none, 1 to 3 for
 * 50%, 75% and 90% of it used, `BILLED` for its limit reached with extra usage paying past it,
 * and `STOPPED` for its limit reached with nothing paying, so that requests are refused.
 */
export type UsageLevel = 0 | 1 | 2 | 3 | 4 | 5;

/** A limit reached, and extra usage pays for the work: it goes on, billed. */
export const BILLED: UsageLevel = 4;

/** A limit reached, and nothing pays: requests are refused until it resets. */
export const STOPPED: UsageLevel = 5;

/** The shares of a window used that are a level each, lowest first. */
const SHARES = [0.5, 0.75, 0.9];

/**
 * A window's level, `paying` saying whether extra usage pays past a limit reached. A report's
 * own word for how near it is is not taken, as in `limitLevel`.
 */
export function windowLevel(window: LimitWindow, paying: boolean): UsageLevel {
    if (limitLevel(window) === "reached") {
        return paying ? BILLED : STOPPED;
    }
    const passed = SHARES.filter((share) => window.used >= share).length;
    return passed === 3 ? 3 : passed === 2 ? 2 : passed === 1 ? 1 : 0;
}

/** The lowest level the setting says: every one when frequent, from 90% when rare, and only requests refused when none. */
export function lowestSaid(mode: LimitWarnings): UsageLevel {
    switch (mode) {
        case "frequent":
            return 1;
        case "rare":
            return 3;
        case "none":
            return STOPPED;
    }
}

/** The usage notice: the window at the highest level, and that level. */
export interface UsageNotice {
    window: LimitWindow;
    level: UsageLevel;
}

/**
 * What dismissing the usage notice keeps, for the account and not for a session: the highest
 * level any window had reached then, and when each window was to reset, by its kind.
 */
export interface Dismissed {
    level: UsageLevel;
    resets: Record<string, number | null>;
}

/** What dismissing the notice keeps, with `windows` as they stand. */
export function dismissalOf(windows: LimitWindow[], paying: boolean): Dismissed {
    const level = windows.reduce<UsageLevel>(
        (highest, window) => Math.max(highest, windowLevel(window, paying)) as UsageLevel,
        0,
    );
    return { level, resets: Object.fromEntries(windows.map((w) => [w.kind, w.resetsAt])) };
}

/**
 * Whether a dismissal keeps a window's notice away at `now`: the window was known when the
 * notice was dismissed, is at no higher level than the one dismissed, and has not reset since.
 * A window that was to reset before `now` has, and whatever level it climbs back to is news.
 * So is one nobody knew of then, such as a model's weekly limit first reported later: what was
 * dismissed did not speak of it. One known with no reset is kept away until a higher level,
 * since there is no telling when it resets: were it news, dismissing it would do nothing while
 * reports leave its reset out.
 */
export function heldBack(
    window: LimitWindow,
    level: UsageLevel,
    dismissed: Dismissed | null,
    now: number,
): boolean {
    if (dismissed === null || level > dismissed.level) {
        return false;
    }
    const reset = dismissed.resets[window.kind];
    if (reset === undefined) {
        return false;
    }
    return reset === null || reset > now;
}

/**
 * The usage notice to show at `now` (seconds since 1970), or `null` for none: of the windows
 * at a level `mode` says, and not kept away by `dismissed`, the one at the highest level, and
 * of those the fullest. Built from the account's windows, so reports of one window and another
 * in turn change nothing: a report can only raise or lower the level.
 */
export function usageNotice(
    windows: LimitWindow[],
    paying: boolean,
    mode: LimitWarnings,
    dismissed: Dismissed | null,
    now: number,
): UsageNotice | null {
    const lowest = lowestSaid(mode);
    const said = windows
        .map((window) => ({ window, level: windowLevel(window, paying) }))
        .filter(({ level }) => level > 0 && level >= lowest)
        .filter(({ window, level }) => !heldBack(window, level, dismissed, now));
    const above = (a: UsageNotice, b: UsageNotice) =>
        a.level > b.level || (a.level === b.level && a.window.used > b.window.used);
    return said.reduce<UsageNotice | null>(
        (best, notice) => (best === null || above(notice, best) ? notice : best),
        null,
    );
}

/**
 * What the usage notice says, in a sentence or two. A model's own limit stops that model
 * alone, and an answer's 100% that no report called reached is not used up yet.
 */
export function usageNoticeText({ window, level }: UsageNotice, now: Date): string {
    const resets = window.resetsAt === null ? "" : ` It resets ${resetWhen(window.resetsAt, now)}.`;
    if (level === STOPPED) {
        const refused = oneModels(window)
            ? "Claude Code's requests to that model are refused, and other models go on"
            : "Claude Code's requests are refused";
        return `The ${window.name} limit is reached, and nothing pays past it: ${refused}.${resets}`;
    }
    if (level === BILLED) {
        return `The ${window.name} limit is reached: Claude Code goes on with extra usage, which is billed.${resets}`;
    }
    const share = Math.round(window.used * 100);
    const used = share >= 100 ? "all but used up" : `${share}% used`;
    return `The ${window.name} limit is ${used}. Nothing is limited yet.${resets}`;
}

/** A dismissal as it was kept on this machine, or `null` if what is kept is not one. */
export function readDismissed(stored: unknown): Dismissed | null {
    if (!(isRecord(stored) && isRecord(stored.resets))) {
        return null;
    }
    const { level } = stored;
    const isLevel = (value: unknown): value is UsageLevel =>
        typeof value === "number" && Number.isInteger(value) && value >= 0 && value <= STOPPED;
    if (!isLevel(level)) {
        return null;
    }
    const resets: Record<string, number | null> = {};
    for (const [kind, at] of Object.entries(stored.resets)) {
        if (typeof at === "number" || at === null) {
            resets[kind] = at;
        }
    }
    return { level, resets };
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
            kind: window.kind,
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

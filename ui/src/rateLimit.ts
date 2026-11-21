// What Claude Code reports about the user's usage limits, in words for the transcript. It
// reports after every turn: `allowed`, `allowed_warning` while a window fills up, and another
// status once a limit is reached. A warning is not a limit reached, and is said once, when it
// starts, not again with every turn.

import type { AgentEvent, UsageLimit } from "./api/types";

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
export function onExtraUsage(event: RateLimit): boolean {
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
 * well, a warning when one starts, and every time a limit is reached. `previous` is the report
 * before it in the conversation.
 */
export function limitNotice(
    event: RateLimit,
    previous: RateLimit | null,
    now: Date,
): string | null {
    if (event.status === "allowed") {
        return null;
    }
    const { window, used } = details(event);
    const resets = event.resets_at === null ? "" : ` It resets ${resetWhen(event.resets_at, now)}.`;
    if (onExtraUsage(event)) {
        return billedNotice(previous, window, resets);
    }
    if (event.status !== "allowed_warning") {
        return `Usage limit reached${window ? ` (${window})` : ""}.${resets}`;
    }
    const warnedAlready =
        previous?.status === "allowed_warning" && details(previous).window === window;
    if (warnedAlready) {
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

/** One of Claude Code's usage windows, as its last report gave it. */
export interface LimitWindow {
    /** What the window is called here, such as "5-hour" or "weekly". */
    name: string;
    /** How much of it is used, from 0 to 1. */
    used: number;
    /** When it resets, in seconds since 1970; `null` if not said. */
    resetsAt: number | null;
    /** `allowed`, or what stops the next request. */
    status: string;
}

/**
 * Every usage window, from whichever came last: the report after a turn, or the limits Claude
 * Code gave when asked for them. In the order Claude Code lists them. Only Claude Code reports
 * them: the Agent Client Protocol has no message for an account's limits.
 */
export function limitWindows(events: AgentEvent[]): LimitWindow[] {
    for (let at = events.length - 1; at >= 0; at--) {
        const windows = windowsOf(events[at]);
        if (windows.length > 0) {
            return windows;
        }
    }
    return [];
}

function windowsOf(event: AgentEvent | undefined): LimitWindow[] {
    if (event?.type === "usage_limits") {
        return event.limits.map(askedWindow);
    }
    return event?.type === "rate_limit" ? reportedWindows(event) : [];
}

/** What Claude Code's `/usage` calls its limits, and what they are called here. */
const LIMIT_KINDS: Record<string, string> = {
    session: "5-hour",
    weekly_all: "weekly",
    weekly_scoped: "weekly",
};

/** A limit from Claude Code's answer, as a window: one model's weekly limit is named after it. */
function askedWindow(limit: UsageLimit): LimitWindow {
    const kind = LIMIT_KINDS[limit.kind] ?? limit.kind.replaceAll("_", " ");
    return {
        name: limit.model ? `${kind} ${limit.model}` : kind,
        used: limit.percent / 100,
        resetsAt: isoSeconds(limit.resets_at),
        status: limit.severity === "normal" ? "allowed" : limit.severity,
    };
}

/**
 * An ISO 8601 time as seconds since 1970; `null` for none, or one that cannot be read. Claude
 * Code gives microseconds, which are cut to milliseconds first: not every engine reads more.
 */
export function isoSeconds(text: string | null): number | null {
    if (text === null) {
        return null;
    }
    const ms = Date.parse(text.replace(/(\.\d{3})\d+/, "$1"));
    return Number.isNaN(ms) ? null : ms / 1000;
}

/** The windows a report after a turn lists. */
function reportedWindows(report: RateLimit): LimitWindow[] {
    if (!isRecord(report.raw)) {
        return [];
    }
    const windows = report.raw.unifiedWindows;
    if (!isRecord(windows)) {
        return [];
    }
    return Object.entries(windows).flatMap(([type, window]): LimitWindow[] => {
        if (!isRecord(window) || typeof window.utilization !== "number") {
            return [];
        }
        return [
            {
                name: windowName(type),
                used: window.utilization,
                resetsAt: typeof window.resetsAt === "number" ? window.resetsAt : null,
                status: typeof window.status === "string" ? window.status : "allowed",
            },
        ];
    });
}

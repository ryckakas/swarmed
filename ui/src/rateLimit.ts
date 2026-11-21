// What Claude Code reports about the user's usage limits, in words for the transcript. It
// reports after every turn: `allowed`, `allowed_warning` while a window fills up, and another
// status once a limit is reached. A warning is not a limit reached, and is said once, when it
// starts, not again with every turn.

import type { AgentEvent } from "./api/types";

/** One usage report of Claude Code's. */
export type RateLimit = Extract<AgentEvent, { type: "rate_limit" }>;

/** What Claude Code calls its usage windows, and what they are called here. */
const WINDOWS: Record<string, string> = {
    five_hour: "5-hour",
    seven_day: "weekly",
    seven_day_opus: "weekly Opus",
    seven_day_sonnet: "weekly Sonnet",
};

function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null;
}

/** Which window a report is about, and how much of it is used, where Claude Code says. */
function details(event: RateLimit): { window: string | null; used: number | null } {
    const raw = isRecord(event.raw) ? event.raw : {};
    const type = raw.rateLimitType;
    const window = typeof type === "string" ? (WINDOWS[type] ?? type.replaceAll("_", " ")) : null;
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

// The account's extra usage: what is spent beyond the plan's limits this billing period,
// billed at API prices up to a monthly cap the user sets. Claude Code is asked for it when a
// session starts and whenever a turn runs on it; other agents have nothing like it.

import type { AgentEvent } from "./api/types";
import { limitLevel, limitWindows, onExtraUsage } from "./rateLimit";

/** What extra usage stood at when Claude Code was last asked. */
export type ExtraUsage = Extract<AgentEvent, { type: "extra_usage" }>;

/** The latest figures in the session, or `null` if Claude Code was never asked. */
export function latestExtraUsage(events: AgentEvent[]): ExtraUsage | null {
    const latest = events.findLast((event) => event.type === "extra_usage");
    return latest?.type === "extra_usage" ? latest : null;
}

/**
 * Whether the session's turns run on extra usage, billed: so its latest usage report said, and
 * no limits Claude Code was asked for since say otherwise. Only a turn sends a report, so after
 * a window resets the report still says billed; the limits asked for then, from the coins, show
 * every window back within, and that is what holds until the next turn.
 */
export function onExtraUsageNow(events: AgentEvent[]): boolean {
    const at = events.findLastIndex((event) => event.type === "rate_limit");
    const latest = events[at];
    if (latest?.type !== "rate_limit" || !onExtraUsage(latest)) {
        return false;
    }
    const since = limitWindows(events.slice(at + 1));
    return since.length === 0 || since.some((window) => limitLevel(window) === "reached");
}

/** An amount in minor units, as money in its currency: 3871 cents of euros is €38.71. */
export function money(minor: number, currency: string, decimals: number): string {
    const amount = minor / 10 ** decimals;
    try {
        return new Intl.NumberFormat(undefined, {
            style: "currency",
            currency,
            minimumFractionDigits: decimals,
            maximumFractionDigits: decimals,
        }).format(amount);
    } catch {
        // A currency code the platform does not know is still worth showing.
        return `${amount.toFixed(decimals)} ${currency}`;
    }
}

/** How much of the cap is spent, 0 to 100; `null` without a cap. */
export function spentShare(usage: ExtraUsage): number | null {
    if (usage.limit === null || usage.limit === 0) {
        return null;
    }
    return Math.min(100, Math.round((usage.used / usage.limit) * 100));
}

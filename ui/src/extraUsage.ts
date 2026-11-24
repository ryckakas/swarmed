// The account's extra usage: what is spent beyond the plan's limits this billing period,
// billed at API prices up to a monthly cap the user sets. Claude Code is asked for it when a
// session starts and whenever a turn runs on it; other agents have nothing like it. What any
// Claude Code session heard last is what every one shows.

import type { AccountUsage, ExtraUsageHeard } from "./api/types";
import { accountWindows, limitLevel } from "./rateLimit";

/** What extra usage stood at when Claude Code was last asked. */
export type ExtraUsage = ExtraUsageHeard;

/**
 * Whether turns run on extra usage, billed, at `now` (seconds since 1970): so the latest report
 * after a turn said, the limit it was about has not reset since, and no limits Claude Code was
 * asked for since say otherwise. Only a turn sends a report, so after a window resets the
 * report still says billed; the limits asked for then, from the coins, show every window back
 * within, and that is what holds until the next turn.
 */
export function onExtraUsageNow(usage: AccountUsage | null, now: number): boolean {
    const billed = usage?.billed;
    if (!billed?.on || (billed.resets_at !== null && billed.resets_at <= now)) {
        return false;
    }
    const askedSince = accountWindows(usage, now).filter(
        (window) => window.asked && window.heardAt >= billed.heard_at,
    );
    return askedSince.length === 0 || askedSince.some((window) => limitLevel(window) === "reached");
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

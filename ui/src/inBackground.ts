// What the sidebar's marks say of work a session's agent left running in the background
// (#420). Pure, so it can be tested.

import type { BackgroundCount, SessionStatus } from "./api/types";

/**
 * How much a mark says of what runs in the background: sub-agents at work, which the session is
 * not done with, or only commands left running, often on purpose and so said more quietly.
 */
export type BackgroundLevel = "agents" | "commands";

/** Nothing running in the background. */
export const NOTHING_IN_BACKGROUND: BackgroundCount = { agents: 0, commands: 0 };

/** What several sessions have running in the background, added up. */
export function addedUp(counts: BackgroundCount[]): BackgroundCount {
    return counts.reduce(
        (sum, count) => ({
            agents: sum.agents + count.agents,
            commands: sum.commands + count.commands,
        }),
        NOTHING_IN_BACKGROUND,
    );
}

/** Whether anything runs in the background. */
export function anyInBackground(count: BackgroundCount): boolean {
    return count.agents > 0 || count.commands > 0;
}

/**
 * What a status mark shows of the background: only beside idle, since working, waiting and an
 * error already say more, and never nothing while something runs there. Sub-agents rank above
 * commands.
 */
export function backgroundShown(
    status: SessionStatus,
    count: BackgroundCount,
): BackgroundLevel | null {
    if (status !== "idle") {
        return null;
    }
    if (count.agents > 0) {
        return "agents";
    }
    return count.commands > 0 ? "commands" : null;
}

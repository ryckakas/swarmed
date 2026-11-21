// What a session's agent has spent, from what it reports. An agent reports a running total
// since its process started, and starts again from nothing when a session is resumed: what the
// session cost in all is the sum of the steps, as the core's costs count them (`costs.rs`).

import type { AgentEvent } from "./api/types";

/** What the figure is, from where the agent found its credentials. */
export type SpendBasis = "subscription" | "api" | "agent";

/**
 * What the session has spent in all, in US dollars, across every run of its agent; `null` if
 * it never said. A total lower than the last means a run that started unnoticed: it counts
 * whole.
 */
export function sessionSpend(events: AgentEvent[]): number | null {
    let spent = 0;
    let last = 0;
    let reported = false;
    for (const event of events) {
        if (event.type === "init") {
            last = 0;
        } else if (event.type === "turn_done" && event.cost_usd !== null) {
            reported = true;
            spent += event.cost_usd >= last ? event.cost_usd - last : event.cost_usd;
            last = event.cost_usd;
        }
    }
    return reported ? spent : null;
}

/**
 * Whether the spend was paid, as the session's agent last started: Claude Code on a stored
 * subscription login reports what the work would cost at API prices, and on an API key what
 * it was billed. An agent over the Agent Client Protocol does not say how it signed in.
 */
export function spendBasis(events: AgentEvent[]): SpendBasis | null {
    const init = events.findLast((event) => event.type === "init");
    if (init?.type !== "init") {
        return null;
    }
    if (init.api_key_source === "agent") {
        return "agent";
    }
    return init.api_key_source === "none" ? "subscription" : "api";
}

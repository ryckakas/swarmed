// Asking Claude Code for the account's usage, from the overview a session's coins button
// opens. Nothing asks on its own: opening the overview asks, and so does its button. Between
// those, the figures are what Claude Code last said while it worked for the session.

import type { AgentEvent } from "./api/types";

/** How many usage figures a session has heard: a new one is the answer to asking. */
export function usageFigures(events: AgentEvent[]): number {
    return events.filter((event) => event.type === "usage_limits" || event.type === "extra_usage")
        .length;
}

/** What is said when Claude Code gave no figures in time: it gives none to an API key. */
export const NO_FIGURES =
    "Claude Code gave no usage figures: it gives them when signed in to a Claude plan";

/** Where asking stands. */
export type Asked =
    | { state: "asking" }
    | { state: "asked"; at: Date }
    | { state: "failed"; error: string };

/** What the overview says of its figures: being asked for, when they were given, or why not. */
export function askedLine(asked: Asked): string {
    switch (asked.state) {
        case "asking":
            return "Asking Claude Code…";
        case "asked": {
            const time = asked.at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
            return `As Claude Code gave them at ${time}.`;
        }
        case "failed":
            return `${asked.error.replace(/\.$/, "")}. These are from the session's last turn.`;
    }
}

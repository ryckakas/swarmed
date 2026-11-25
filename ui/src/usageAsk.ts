// Asking Claude Code for the account's usage, from the overview a session's coins button
// opens. Nothing asks on its own: opening the overview asks, and so does its button. Between
// those, the figures are the account's as any Claude Code session last heard them. Claude Code
// answers from its own copy of the figures for a minute after it fetched them, so while that
// copy is younger the core holds the question back until it is not (`waiting`), and the answer
// is fetched after the user asked. Each step shows: the question going, the time it goes when
// held back, counting down, and the answer to the second, new or the same as before.

import type { AccountUsage, AgentEvent } from "./api/types";

/** How many usage figures a session has heard: a new one is the answer to asking. */
export function usageFigures(events: AgentEvent[]): number {
    return events.filter((event) => event.type === "usage_limits" || event.type === "extra_usage")
        .length;
}

/** What is said when Claude Code gave no figures in time: it gives none to an API key. */
export const NO_FIGURES =
    "Claude Code gave no usage figures: it gives them when signed in to a Claude plan";

/**
 * Where asking stands: asked, held back until Claude Code's copy of the figures (fetched at
 * `copyFrom`) is old enough for it to fetch them again at `goesAt`, answered, or not.
 */
export type Asked =
    | { state: "asking" }
    | { state: "waiting"; goesAt: Date; copyFrom: Date }
    | { state: "asked"; at: Date }
    | { state: "failed"; error: string };

/** A time to the second, for a wait of under a minute. */
function toTheSecond(at: Date): string {
    return at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

/**
 * What the overview says of its figures at `now`: being asked for, held back with the seconds
 * left, when they were given and whether they are `same` as before asking, or why not.
 */
export function askedLine(asked: Asked, now: Date, same: boolean): string {
    switch (asked.state) {
        case "asking":
            return "Asking Claude Code…";
        case "waiting": {
            const left = Math.ceil((asked.goesAt.getTime() - now.getTime()) / 1000);
            if (left <= 0) {
                return "Asking Claude Code…";
            }
            return `Claude Code reuses the figures it fetched at ${toTheSecond(asked.copyFrom)} for a minute: asking it in ${left} s…`;
        }
        case "asked":
            return `Claude Code gave ${same ? "the same" : "new"} figures at ${toTheSecond(asked.at)}.`;
        case "failed":
            return `${asked.error.replace(/\.$/, "")}. These are the latest any session heard.`;
    }
}

/**
 * The account's figures as the overview shows them, in one string to compare: every window and
 * the extra usage, but not when each was heard, which changes with every answer. A window is
 * taken as it is drawn: its share in whole percent, since a report after a turn gives a
 * fraction and an answer to asking a whole percent of the same usage (0.4263 and 0.43), and
 * whether a report said it is reached, not the words of either for how near it is
 * (`allowed_warning` and `warning`). A reset is taken to the minute: claude.ai gives the same
 * one a fraction of a second either side of it at each fetch, such as 19:59:59.51 and then
 * 20:00:00.09.
 */
export function figuresKey(usage: AccountUsage | null): string {
    const minute = (at: number | null) => (at === null ? null : Math.round(at / 60));
    const windows = (usage?.windows ?? []).map(({ kind, used, resets_at, status }) => [
        kind,
        Math.round(used * 100),
        minute(resets_at),
        status === "rejected",
    ]);
    const extra = usage?.extra ?? null;
    const spent = extra && [extra.enabled, extra.used, extra.limit, extra.limit_reached];
    return JSON.stringify([windows, spent]);
}

/** Whether asking is under way, so that it is not asked again meanwhile. */
export function underWay(asked: Asked | null): boolean {
    return asked?.state === "asking" || asked?.state === "waiting";
}

/**
 * What the session's figures, `figures` of them where there were `from` when the question was
 * asked, say of it at `now` (milliseconds since 1970): no new one (`none`), one that came before
 * the question went and answers something else (`early`), or the answer.
 */
export function answerHeard(
    asked: Asked | null,
    from: number,
    figures: number,
    now: number,
): "none" | "early" | "answer" {
    if (figures <= from) {
        return "none";
    }
    return asked?.state === "waiting" && now < asked.goesAt.getTime() ? "early" : "answer";
}

/** How long to wait for an answer, from `now`: from when the question goes, if it waits. */
export function answerWait(asked: Asked | null, now: number, wait: number): number {
    const goes = asked?.state === "waiting" ? asked.goesAt.getTime() - now : 0;
    return Math.max(0, goes) + wait;
}

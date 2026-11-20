import type { AgentEvent, SessionEvent } from "./api/types";

// How long turns take. Pure, so it can be tested.

/** A length of time as people say it: 8s, 1m 12s, 2h 05m. */
export function formatDuration(ms: number): string {
    const seconds = Math.max(0, Math.round(ms / 1000));
    if (seconds < 60) {
        return `${seconds}s`;
    }
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) {
        return `${minutes}m ${String(seconds % 60).padStart(2, "0")}s`;
    }
    return `${Math.floor(minutes / 60)}h ${String(minutes % 60).padStart(2, "0")}m`;
}

/** How long the session's finished turns took, added up. */
export function timeWorked(events: AgentEvent[]): number {
    return events.reduce(
        (total, event) => total + (event.type === "turn_done" ? (event.duration_ms ?? 0) : 0),
        0,
    );
}

/**
 * When each session's running turn began, as seen from here: a message starts it and the end
 * of the turn, or of the agent, forgets it. The same object comes back if nothing changed.
 */
export function turnsBegan(
    began: Record<number, number>,
    { session_id, event }: SessionEvent,
    now: number,
): Record<number, number> {
    if (event.type === "user_message" && began[session_id] === undefined) {
        return { ...began, [session_id]: now };
    }
    const over = event.type === "turn_done" || event.type === "exited";
    if (over && began[session_id] !== undefined) {
        const { [session_id]: _ended, ...rest } = began;
        return rest;
    }
    return began;
}

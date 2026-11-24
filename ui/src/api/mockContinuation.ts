import type { AgentEvent } from "./types";

// The sample agent's sessions going on by themselves when the usage limit that stopped them
// resets, as the core does it (`continuation.rs`): set when asked, or for every refused turn
// when the setting says so, and sent through the queue at the reset, a few seconds after the
// session continued before it.

/** What a session is sent when the limit that stopped it resets: the core's words. */
export const CONTINUE_MESSAGE = "Continue where you stopped.";

/** How long after continuing one session the next goes, as in the core. */
const APART_MS = 5_000;

/**
 * A turn refused by the 5-hour limit, which resets `inSeconds` from now, as Claude Code
 * reports it: used up, and nothing paying past it.
 */
export function mockRefusal(inSeconds: number): AgentEvent {
    const resetsAt = Math.round(Date.now() / 1000) + inSeconds;
    const window = { status: "rejected", utilization: 1, resetsAt };
    return {
        type: "rate_limit",
        status: "rejected",
        resets_at: resetsAt,
        raw: {
            status: "rejected",
            rateLimitType: "five_hour",
            resetsAt,
            unifiedWindows: { five_hour: window },
        },
    };
}

/** When the limit a usage report says refused the turn resets; `null` for any other event. */
function refusedUntil(event: AgentEvent): number | null {
    if (event.type !== "rate_limit" || ["allowed", "allowed_warning"].includes(event.status)) {
        return null;
    }
    const raw = (typeof event.raw === "object" && event.raw !== null ? event.raw : {}) as Record<
        string,
        unknown
    >;
    return raw.isUsingOverage === true || raw.overageInUse === true ? null : event.resets_at;
}

/**
 * When every limit that refused the turns since the latest message resets, in seconds since
 * 1970, if that is still to come at `now`: what the core waits for.
 */
export function resetToWaitFor(history: AgentEvent[], now: number): number | null {
    const since = history.findLastIndex((event) => event.type === "user_message");
    const resets = history
        .slice(since + 1)
        .map(refusedUntil)
        .filter((at) => at !== null);
    const latest = resets.length === 0 ? null : Math.max(...resets);
    return latest !== null && latest > now ? latest : null;
}

/** The timers that continue each session at its time, one a few seconds after another. */
export class MockContinuations {
    private readonly timers = new Map<number, ReturnType<typeof setTimeout>>();
    private last = 0;
    private readonly go: (id: number) => void;

    constructor(go: (id: number) => void) {
        this.go = go;
    }

    /** Continues session `id` at `at`, in seconds since 1970, or no longer for `null`. */
    schedule(id: number, at: number | null) {
        clearTimeout(this.timers.get(id));
        this.timers.delete(id);
        if (at !== null) {
            this.wait(id, at * 1000 - Date.now());
        }
    }

    private wait(id: number, ms: number) {
        this.timers.set(
            id,
            setTimeout(() => this.due(id), Math.max(0, ms)),
        );
    }

    private due(id: number) {
        const since = Date.now() - this.last;
        if (since < APART_MS) {
            this.wait(id, APART_MS - since);
            return;
        }
        this.last = Date.now();
        this.timers.delete(id);
        this.go(id);
    }
}

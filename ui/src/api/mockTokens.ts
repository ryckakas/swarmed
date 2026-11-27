import type { AgentEvent, TokenPhase, Tokens } from "./types";

// The tokens the sample agent's turns use, published as the core does for Claude Code: a
// request goes out and is read, its answer comes in and its output rises, and between requests
// a tool runs. Only the sample Claude Code says any, as only Claude Code does.

/** One running count, as the core publishes it. */
export interface Count {
    tokens: Tokens;
    phase: TokenPhase;
}

export const NO_TOKENS: Tokens = { input: 0, output: 0 };

/**
 * What the core publishes of one request of a turn that had used `before`: going out, its
 * answer coming in a fifth at a time, then done. `read` is what the model read anew.
 */
export function requestCounts(before: Tokens, read: number): Count[] {
    const input = before.input + read;
    const written = 120 + Math.round(read / 40);
    const coming = [1, 2, 3, 4, 5].map(
        (fifth): Count => ({
            tokens: { input, output: before.output + Math.round((written * fifth) / 5) },
            phase: "response",
        }),
    );
    const after = { input, output: before.output + written };
    return [{ tokens: before, phase: "prompt" }, ...coming, { tokens: after, phase: "between" }];
}

/** A count as the event that carries it: the session's own, or a sub-agent's. */
export function countEvent(count: Count, parent: string | null = null): AgentEvent {
    const event: AgentEvent = { type: "turn_tokens", ...count };
    return parent === null ? event : { type: "sub", parent, event };
}

/** `done` with what its turn used, if it is a turn's end. */
export function withTokens(done: AgentEvent, tokens: Tokens): AgentEvent {
    return done.type === "turn_done" ? { ...done, tokens } : done;
}

/**
 * Keeps a turn's count going, request after request, one count every `pace` milliseconds, as a
 * `slow` turn's would. Returns what stops it, which says what the turn used.
 */
export function keepCounting(show: (event: AgentEvent) => void, pace: number): () => Tokens {
    let used = NO_TOKENS;
    let counts = requestCounts(used, 18_400);
    const timer = setInterval(() => {
        const next = counts.shift();
        if (next === undefined) {
            // A pause between requests, while a tool runs, then the next goes out.
            counts = requestCounts(used, 1_250);
            return;
        }
        used = next.tokens;
        show(countEvent(next));
    }, pace);
    return () => {
        clearInterval(timer);
        return used;
    };
}

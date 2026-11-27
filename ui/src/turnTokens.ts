import type { SessionEvent, TokenPhase, Tokens } from "./api/types";
import { scrollKey, withoutPlaces } from "./scrollSpot";
import { formatTokens } from "./statusline";

// The tokens a turn uses: the running count beside what the agent is doing, and what each
// finished turn used. Pure, so it can be tested.

/** What a turn under way has used so far, and which way its tokens go now. */
export interface RunningTokens {
    tokens: Tokens;
    phase: TokenPhase;
}

/**
 * The running count an event carries, the session's own or one of its sub-agents', with where
 * it is kept: under the key of the transcript it shows in (`scrollKey`). `null` for any other
 * event.
 */
export function runningCount(
    published: SessionEvent,
): { place: string; count: RunningTokens } | null {
    const { session_id, event } = published;
    if (event.type === "turn_tokens") {
        const count = { tokens: event.tokens, phase: event.phase };
        return { place: scrollKey(session_id), count };
    }
    if (event.type === "sub" && event.event.type === "turn_tokens") {
        const count = { tokens: event.event.tokens, phase: event.event.phase };
        return { place: scrollKey(session_id, event.parent), count };
    }
    return null;
}

/**
 * The running counts after an event: the one it carries in place of the one kept before, the
 * session's own gone once its turn ends, and every one of the session's once its agent has
 * ended. A sub-agent's outlasts the turn, as one in the background works on after it. The
 * same object if nothing changed.
 */
export function withTurnTokens(
    all: Record<string, RunningTokens>,
    published: SessionEvent,
): Record<string, RunningTokens> {
    const running = runningCount(published);
    if (running !== null) {
        return { ...all, [running.place]: running.count };
    }
    const { session_id, event } = published;
    if (event.type === "exited") {
        return withoutPlaces(all, [session_id]);
    }
    const own = scrollKey(session_id);
    if (event.type === "turn_done" && all[own] !== undefined) {
        const { [own]: _ended, ...rest } = all;
        return rest;
    }
    return all;
}

/** All the tokens, as said beside a turn: `3.4k tokens`. */
export function tokensSaid(tokens: Tokens): string {
    return `${formatTokens(tokens.input + tokens.output)} tokens`;
}

/** The tokens apart: `43.8k read anew, 97 written`. */
export function tokensApart(tokens: Tokens): string {
    return `${formatTokens(tokens.input)} read anew, ${formatTokens(tokens.output)} written`;
}

/** Which way a running turn's tokens go, in words. */
export const PHASE_SAID: Record<TokenPhase, string> = {
    prompt: "Sending a request",
    response: "Receiving the answer",
    between: "Between requests",
};

/** What a running count says when pointed at: which way, and the tokens apart. */
export function runningSaid({ tokens, phase }: RunningTokens): string {
    return `${PHASE_SAID[phase]}. So far: ${tokensApart(tokens)}.`;
}

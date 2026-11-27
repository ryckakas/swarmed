import { describe, expect, it } from "vitest";
import type { AgentEvent, SessionEvent, TokenPhase } from "./api/types";
import { runningCount, runningSaid, tokensApart, tokensSaid, withTurnTokens } from "./turnTokens";

const published = (event: AgentEvent, session_id = 7): SessionEvent => ({
    session_id,
    status: null,
    event,
});

const running = (input: number, output: number, phase: TokenPhase): AgentEvent => ({
    type: "turn_tokens",
    tokens: { input, output },
    phase,
});

const ofSub = (parent: string, event: AgentEvent): AgentEvent => ({ type: "sub", parent, event });

const done: AgentEvent = { type: "turn_done", is_error: false, result: null, cost_usd: null };

describe("the running counts", () => {
    it("are kept where their transcript is: the session's, or a sub-agent's under it", () => {
        expect(runningCount(published(running(10, 2, "prompt")))).toEqual({
            place: "7",
            count: { tokens: { input: 10, output: 2 }, phase: "prompt" },
        });
        expect(runningCount(published(ofSub("toolu_1", running(5, 1, "response"))))).toEqual({
            place: "7/toolu_1",
            count: { tokens: { input: 5, output: 1 }, phase: "response" },
        });
        expect(runningCount(published({ type: "text", text: "hi" }))).toBeNull();
        expect(runningCount(published(ofSub("toolu_1", { type: "text", text: "hi" })))).toBeNull();
    });

    it("take each other's place, and go with the turn or, a sub-agent's too, with the agent", () => {
        let all = withTurnTokens({}, published(running(10, 2, "prompt")));
        all = withTurnTokens(all, published(running(900, 40, "response")));
        all = withTurnTokens(all, published(ofSub("toolu_1", running(5, 1, "response"))));
        all = withTurnTokens(all, published(running(3, 3, "prompt"), 8));
        expect(all).toEqual({
            "7": { tokens: { input: 900, output: 40 }, phase: "response" },
            "7/toolu_1": { tokens: { input: 5, output: 1 }, phase: "response" },
            "8": { tokens: { input: 3, output: 3 }, phase: "prompt" },
        });
        // A sub-agent in the background counts on after its starter's turn.
        const ended = withTurnTokens(all, published(done));
        expect(Object.keys(ended).toSorted()).toEqual(["7/toolu_1", "8"]);
        const gone = withTurnTokens(ended, published({ type: "exited", code: 0, stderr: "" }));
        expect(Object.keys(gone)).toEqual(["8"]);
    });

    it("stay the same object for anything that changes none of them", () => {
        const all = withTurnTokens({}, published(running(10, 2, "prompt")));
        expect(withTurnTokens(all, published({ type: "text", text: "hi" }))).toBe(all);
        expect(withTurnTokens(all, published(done, 8))).toBe(all);
        expect(withTurnTokens(all, published({ type: "exited", code: 0, stderr: "" }, 8))).toBe(
            all,
        );
    });
});

describe("what the tokens say", () => {
    it("is all of them beside a turn, and apart when pointed at", () => {
        expect(tokensSaid({ input: 43_790, output: 97 })).toBe("43.9k tokens");
        expect(tokensSaid({ input: 0, output: 0 })).toBe("0 tokens");
        expect(tokensApart({ input: 43_790, output: 97 })).toBe("43.8k read anew, 97 written");
        expect(runningSaid({ tokens: { input: 1200, output: 340 }, phase: "response" })).toBe(
            "Receiving the answer. So far: 1.2k read anew, 340 written.",
        );
    });
});

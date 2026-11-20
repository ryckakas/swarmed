import { describe, expect, it } from "vitest";
import type { AgentEvent, SessionEvent } from "./api/types";
import { formatDuration, timeWorked, turnsBegan } from "./turns";

const done = (ms: number | null): AgentEvent => ({
    type: "turn_done",
    is_error: false,
    result: null,
    cost_usd: null,
    duration_ms: ms,
});

describe("formatDuration", () => {
    it("says seconds, then minutes and seconds, then hours and minutes", () => {
        expect(formatDuration(400)).toBe("0s");
        expect(formatDuration(8_400)).toBe("8s");
        expect(formatDuration(72_000)).toBe("1m 12s");
        expect(formatDuration(60_000)).toBe("1m 00s");
        expect(formatDuration(2 * 3_600_000 + 5 * 60_000)).toBe("2h 05m");
    });
});

describe("timeWorked", () => {
    it("adds up the turns that say how long they took", () => {
        const old: AgentEvent = { type: "turn_done", is_error: false, result: null, cost_usd: 1 };
        expect(timeWorked([done(1000), old, done(null), done(2500)])).toBe(3500);
        expect(timeWorked([])).toBe(0);
    });
});

describe("turnsBegan", () => {
    const at = (event: AgentEvent): SessionEvent => ({ session_id: 7, status: null, event });

    it("starts the clock at a message and stops it when the turn or the agent ends", () => {
        const began = turnsBegan({}, at({ type: "user_message", text: "go" }), 100);
        expect(began).toEqual({ 7: 100 });
        // A message sent into a running turn does not restart it.
        expect(turnsBegan(began, at({ type: "user_message", text: "and" }), 900)).toBe(began);
        expect(turnsBegan(began, at(done(5)), 950)).toEqual({});
        expect(turnsBegan(began, at({ type: "exited", code: 1, stderr: "" }), 950)).toEqual({});
    });

    it("hands back the same record when nothing changed", () => {
        const began = { 3: 1 };
        expect(turnsBegan(began, at({ type: "text", text: "hi" }), 5)).toBe(began);
        expect(turnsBegan(began, at(done(5)), 5)).toBe(began);
    });
});

import { describe, expect, it } from "vitest";
import type { AgentEvent } from "./api/types";
import { suggestionOf, takesSuggestion } from "./suggestion";

const done: AgentEvent = { type: "turn_done", is_error: false, result: null, cost_usd: 0 };
const suggested: AgentEvent = { type: "suggestion", text: "run the tests" };
const said: AgentEvent = { type: "user_message", text: "something else" };

describe("suggestionOf", () => {
    it("gives what the agent suggested after its last turn", () => {
        expect(suggestionOf([said, done, suggested])).toBe("run the tests");
        // What is reported beside the conversation does not move it on.
        const applied: AgentEvent = { type: "applied", model: "m", effort: null };
        expect(suggestionOf([said, done, suggested, applied])).toBe("run the tests");
    });

    it("gives nothing once the conversation has moved on, or without a suggestion", () => {
        expect(suggestionOf([done, suggested, said])).toBeNull();
        // One that came too late, after the next message, is gone when that turn ends.
        expect(suggestionOf([done, said, suggested, done])).toBeNull();
        expect(suggestionOf([said, done])).toBeNull();
        expect(suggestionOf(undefined)).toBeNull();
    });
});

describe("takesSuggestion", () => {
    const tab = { key: "Tab", shiftKey: false };

    it("is Tab on an empty box that has a suggestion", () => {
        expect(takesSuggestion(tab, "", "run the tests")).toBe(true);
        expect(takesSuggestion(tab, "typed", "run the tests")).toBe(false);
        expect(takesSuggestion(tab, "", null)).toBe(false);
        expect(takesSuggestion({ key: "Tab", shiftKey: true }, "", "run the tests")).toBe(false);
        expect(takesSuggestion({ key: "Enter", shiftKey: false }, "", "run the tests")).toBe(false);
    });
});

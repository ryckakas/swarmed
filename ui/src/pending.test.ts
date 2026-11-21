import { describe, expect, it } from "vitest";
import type { AgentEvent } from "./api/types";
import { shortcutRequest } from "./pending";
import { SKIPPED } from "./questions";
import { toItems } from "./transcript";

const request = (requestId: string, toolName: string, input: unknown): AgentEvent => ({
    type: "permission_request",
    request_id: requestId,
    tool_name: toolName,
    input,
    description: null,
});
const push = request("r1", "Bash", { command: "git push" });
const question = request("q1", "AskUserQuestion", {
    questions: [{ question: "Which one?", header: "", options: [{ label: "This" }] }],
});

describe("shortcutRequest", () => {
    it("is the last request still open, for allowing and for denying", () => {
        const items = toItems([push, request("r2", "Bash", { command: "ls" })]);
        expect(shortcutRequest(items, true)?.requestId).toBe("r2");
        expect(shortcutRequest(items, false)).toEqual({
            requestId: "r2",
            refusal: "The user declined this action.",
        });
    });

    it("is none once the request is answered, or while there is none", () => {
        const answered: AgentEvent = {
            type: "permission_answered",
            request_id: "r1",
            allowed: true,
        };
        expect(shortcutRequest(toItems([push, answered]), true)).toBeNull();
        expect(shortcutRequest(toItems([]), false)).toBeNull();
    });

    it("never allows a question, which would answer it with nothing, and may skip it", () => {
        const items = toItems([question]);
        expect(shortcutRequest(items, true)).toBeNull();
        expect(shortcutRequest(items, false)).toEqual({ requestId: "q1", refusal: SKIPPED });
    });

    it("allows a question that cannot be shown as one, as the plain prompt does", () => {
        const items = toItems([request("q2", "AskUserQuestion", { questions: [] })]);
        expect(shortcutRequest(items, true)?.requestId).toBe("q2");
    });
});

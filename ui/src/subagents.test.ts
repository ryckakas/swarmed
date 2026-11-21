import { describe, expect, it } from "vitest";
import type { AgentEvent } from "./api/types";
import { answerOfSubAgent, atWorkIn, eventsOfSubAgent, subAgentOf } from "./subagents";

const start = (id: string, description: string, name = "Agent"): AgentEvent => ({
    type: "tool_use",
    id,
    name,
    input: { description, subagent_type: "Explore", prompt: `Please ${description}` },
});
const result = (id: string, content = "done"): AgentEvent => ({
    type: "tool_result",
    tool_use_id: id,
    content,
    is_error: false,
});
const said = (parent: string, text: string): AgentEvent => ({
    type: "sub",
    parent,
    event: { type: "text", text },
});
const done: AgentEvent = {
    type: "turn_done",
    is_error: false,
    result: null,
    cost_usd: null,
    duration_ms: null,
};

describe("sub-agents at work", () => {
    it("start with their tool call and are done with its result", () => {
        const events = [start("a", "find the routes"), start("b", "read the tests", "Task")];
        expect(atWorkIn(events).map((one) => one.name)).toEqual([
            "find the routes",
            "read the tests",
        ]);
        expect(atWorkIn([...events, result("a")]).map((one) => one.id)).toEqual(["b"]);
        // Another tool's call and result are nothing to them.
        const bash: AgentEvent = { type: "tool_use", id: "c", name: "Bash", input: {} };
        expect(atWorkIn([...events, bash, result("c")])).toHaveLength(2);
    });

    it("do not outlast the turn", () => {
        expect(atWorkIn([start("a", "find the routes"), done])).toEqual([]);
    });

    it("are named by their kind when they are given no description", () => {
        const bare: AgentEvent = {
            type: "tool_use",
            id: "a",
            name: "Agent",
            input: { subagent_type: "Explore" },
        };
        expect(atWorkIn([bare])[0]?.name).toBe("Explore");
        expect(atWorkIn([{ ...bare, input: null }])[0]?.name).toBe("Sub-agent");
    });
});

describe("one sub-agent", () => {
    const events = [
        start("a", "find the routes"),
        start("b", "read the tests"),
        said("a", "Looking."),
        said("b", "Reading."),
        said("a", "Found three."),
        result("a", "Three routes."),
    ];

    it("is known by the call that started it, at work or done", () => {
        expect(subAgentOf(events, "a")).toMatchObject({
            name: "find the routes",
            kind: "Explore",
            prompt: "Please find the routes",
        });
        expect(subAgentOf(events, "nobody")).toBeNull();
    });

    it("has its own events, and its answer once it is done", () => {
        expect(eventsOfSubAgent(events, "a")).toEqual([
            { type: "text", text: "Looking." },
            { type: "text", text: "Found three." },
        ]);
        expect(answerOfSubAgent(events, "a")).toBe("Three routes.");
        expect(answerOfSubAgent(events, "b")).toBeNull();
    });
});

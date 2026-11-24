import { describe, expect, it } from "vitest";
import type { AgentEvent } from "./api/types";
import {
    answerOfSubAgent,
    atWorkIn,
    eventsOfSubAgent,
    isAtWork,
    listedUnder,
    subAgentOf,
    subAgentsIn,
} from "./subagents";

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
        expect(atWorkIn([{ ...bare, input: null }])[0]?.name).toBe("");
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

describe("a sub-agent in the background", () => {
    const report = (
        id: string,
        state: "running" | "completed" | "stopped",
        summary: string | null = null,
    ): AgentEvent => ({
        type: "background",
        task_id: `task-${id}`,
        description: "review",
        kind: "local_agent",
        state,
        summary,
        tool_use_id: id,
        output_file: null,
    });
    const exited: AgentEvent = { type: "exited", code: 0, stderr: "" };
    const launched = result("a", "Async agent launched successfully.");

    it("stays at work past its call's result and the turn, whichever report comes first", () => {
        for (const opening of [
            [start("a", "review"), report("a", "running"), launched],
            [start("a", "review"), launched, report("a", "running")],
        ]) {
            expect(atWorkIn([...opening, done]).map((one) => one.id)).toEqual(["a"]);
        }
    });

    it("leaves on the report of its end, or with the agent", () => {
        const working = [start("a", "review"), report("a", "running"), launched, done];
        expect(atWorkIn([...working, said("a", "Still at it.")])).toHaveLength(1);
        expect(atWorkIn([...working, report("a", "completed", "Found two things.")])).toEqual([]);
        expect(atWorkIn([...working, exited])).toEqual([]);
    });

    it("is listed under an idle session, where one in the foreground is not", () => {
        const events = [start("a", "review"), report("a", "running"), launched, start("b", "find")];
        const followed = subAgentsIn(events);
        expect(listedUnder(followed, "working").map((one) => one.id)).toEqual(["a", "b"]);
        expect(listedUnder(followed, "idle").map((one) => one.id)).toEqual(["a"]);
        expect(listedUnder(followed, "errored")).toEqual([]);
        expect(isAtWork(events, "a", "idle")).toBe(true);
        expect(isAtWork(events, "b", "idle")).toBe(false);
    });

    it("answers with its end report, not with the result that it started", () => {
        const events = [start("a", "review"), report("a", "running"), launched];
        expect(answerOfSubAgent(events, "a")).toBeNull();
        const ended = [...events, report("a", "completed", "Found two things.")];
        expect(answerOfSubAgent(ended, "a")).toBe("Found two things.");
    });

    it("can be one that worked in the foreground first and was sent there", () => {
        const waited = [start("a", "review"), said("a", "Reading.")];
        const moved: AgentEvent = {
            type: "background",
            task_id: "task-a",
            description: "review",
            kind: "local_agent",
            state: "running",
            summary: null,
            tool_use_id: "a",
            output_file: null,
            moved: true,
        };
        // Its call returns as it moves, and the turn goes on and ends: it works on.
        const events = [...waited, moved, result("a", "Sent to the background."), done];
        expect(listedUnder(subAgentsIn(events), "idle").map((one) => one.id)).toEqual(["a"]);
        expect(atWorkIn([...events, report("a", "completed", "Found it.")])).toEqual([]);
    });
});

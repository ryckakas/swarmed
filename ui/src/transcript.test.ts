import { describe, expect, it } from "vitest";
import type { AgentEvent } from "./api/types";
import {
    activity,
    fileName,
    messageNumber,
    planOf,
    sameToolCall,
    splitFiles,
    summarize,
    toItems,
    withFiles,
} from "./transcript";

const toolUse = (id: string, command: string): AgentEvent => ({
    type: "tool_use",
    id,
    name: "Bash",
    input: { command },
});
const toolResult = (id: string, content: string, isError = false): AgentEvent => ({
    type: "tool_result",
    tool_use_id: id,
    content,
    is_error: isError,
});
const request = (requestId: string): AgentEvent => ({
    type: "permission_request",
    request_id: requestId,
    tool_name: "Bash",
    input: { command: "git push" },
    description: null,
});

describe("toItems", () => {
    it("joins a tool result to its call, even with other events in between", () => {
        const items = toItems([
            toolUse("a", "ls"),
            toolUse("b", "pwd"),
            { type: "text", text: "looking" },
            toolResult("b", "/tmp"),
            toolResult("a", "boom", true),
        ]);
        expect(items.map((item) => item.kind)).toEqual(["tool", "tool", "text"]);
        expect(items[0]).toMatchObject({ result: { content: "boom", isError: true } });
        expect(items[1]).toMatchObject({ result: { content: "/tmp", isError: false } });
    });

    it("keeps a proposed change where it was made, and says how it was answered", () => {
        const proposed: AgentEvent = {
            type: "config_proposed",
            request_id: "c1",
            title: "Add this rule for every agent?",
            detail: "never use em dashes",
        };
        const waiting = toItems([proposed, { type: "text", text: "It waits for you." }]);
        expect(waiting[0]).toMatchObject({ kind: "proposal", requestId: "c1", accepted: null });
        // The end of a turn closes permission requests, not proposals.
        const done: AgentEvent = { type: "user_message", text: "thanks" };
        expect(toItems([proposed, done])[0]).toMatchObject({ kind: "proposal", accepted: null });
        const answered = toItems([
            proposed,
            { type: "config_answered", request_id: "c1", accepted: true },
            { type: "config_changed", kind: "rule" },
        ]);
        expect(answered).toHaveLength(1);
        expect(answered[0]).toMatchObject({ kind: "proposal", accepted: true });
    });

    it("shows a call without a result yet as running", () => {
        expect(toItems([toolUse("a", "sleep 60")])[0]).toMatchObject({ result: null });
    });

    const answered = (requestId: string, allowed: boolean): AgentEvent => ({
        type: "permission_answered",
        request_id: requestId,
        allowed,
    });
    const kinds = (events: AgentEvent[]) => toItems(events).map((item) => item.kind);

    it("shows a permission request until it is answered, not until some tool finishes", () => {
        const asked = [toolUse("a", "ls"), request("r1"), toolUse("b", "pwd"), request("r2")];
        expect(kinds(asked)).toEqual(["tool", "permission", "tool", "permission"]);

        // The first command finished; the second request still needs its answer.
        const oneDone = [...asked, answered("r1", true), toolResult("a", "ok")];
        expect(kinds(oneDone)).toEqual(["tool", "tool", "permission"]);
        expect(toItems(oneDone)[2]).toMatchObject({ requestId: "r2" });
    });

    it("leaves nothing behind for an allowed request and one line for a denied one", () => {
        const asked = [toolUse("a", "git push"), request("r1")];
        expect(kinds([...asked, answered("r1", true)])).toEqual(["tool"]);
        expect(kinds([...asked, { type: "permission_withdrawn", request_id: "r1" }])).toEqual([
            "tool",
        ]);
        const denied = toItems([...asked, answered("r1", false)]);
        expect(denied[1]).toMatchObject({ kind: "denied", toolName: "Bash", key: 2 });
    });

    it("treats a request as settled once the turn is over, as old histories need", () => {
        const done: AgentEvent = { type: "turn_done", is_error: false, result: null, cost_usd: 0 };
        expect(kinds([request("r1"), done])).toEqual(["turn"]);
    });

    it("leaves out what is not worth a line", () => {
        const items = toItems([
            { type: "rate_limit", status: "allowed", resets_at: null, raw: {} },
            { type: "exited", code: 0, stderr: "" },
            { type: "exited", code: null, stderr: "" },
            { type: "other", raw: { type: "from_the_future" } },
        ]);
        expect(items).toEqual([]);
    });

    it("reports a failed exit and a reached usage limit", () => {
        const items = toItems([
            { type: "rate_limit", status: "rejected", resets_at: null, raw: {} },
            { type: "exited", code: 1, stderr: "  no such model\n" },
        ]);
        expect(items).toMatchObject([
            { kind: "notice", tone: "warning", text: "Usage limit reached." },
            {
                kind: "notice",
                tone: "error",
                text: "The agent stopped with exit code 1.\nno such model",
            },
        ]);
    });

    it("describes what a session loaded", () => {
        const [item] = toItems([
            {
                type: "init",
                agent_session_id: "abc",
                model: "claude-haiku-5-5",
                permission_mode: "acceptEdits",
                api_key_source: "none",
                mcp_servers: [
                    { name: "context7", status: "connected" },
                    { name: "postgres", status: "failed" },
                ],
                skills: 89,
                slash_commands: [],
            },
        ]);
        expect(item).toMatchObject({
            login: "subscription",
            mcp: "1 of 2 MCP servers connected",
            skills: 89,
        });
    });
});

describe("a session on another agent", () => {
    it("says how many MCP servers it was given, since nothing reports which connected", () => {
        const [item] = toItems([
            {
                type: "init",
                agent_session_id: "acp-1",
                model: "openrouter/some/model",
                permission_mode: "ask",
                api_key_source: "agent",
                mcp_servers: [
                    { name: "context7", status: "passed" },
                    { name: "files", status: "passed" },
                ],
                skills: 0,
                slash_commands: [],
            },
        ]);
        expect(item).toMatchObject({ login: "agent", mcp: "2 MCP servers passed to the agent" });
    });
});

describe("activity", () => {
    const said: AgentEvent = { type: "user_message", text: "go" };
    const done: AgentEvent = { type: "turn_done", is_error: false, result: null, cost_usd: 0 };
    const earlier = [said, toolUse("old", "ls"), done];

    it("says the agent is starting until it has shown anything this turn", () => {
        expect(activity([...earlier, said])).toBe("Starting");
    });

    it("names the tool that is running, and thinks between steps", () => {
        const running = [...earlier, said, toolUse("a", "npm test")];
        expect(activity(running)).toBe("Running Bash");
        expect(activity([...running, toolResult("a", "ok")])).toBe("Thinking");
        expect(activity([said, { type: "text", text: "On it." }])).toBe("Thinking");
    });

    it("says a reply is being written while its pieces arrive", () => {
        expect(activity([said, { type: "text_delta", text: "Rain" }])).toBe("Writing");
    });
});

describe("summarize", () => {
    it("prefers the argument that says what the call does", () => {
        expect(summarize({ command: "ls", description: "List files" })).toBe("ls");
        expect(summarize({ file_path: "/a/b.ts", content: "…" })).toBe("/a/b.ts");
    });

    it("falls back to compact JSON and shortens long text", () => {
        expect(summarize({ a: 1 })).toBe('{"a":1}');
        expect(summarize({ command: "x".repeat(500) })).toHaveLength(120);
        expect(summarize(null)).toBe("null");
    });
});

describe("planOf", () => {
    it("reads the plan an agent presents, and is empty for anything else", () => {
        expect(planOf({ plan: "1. Do it" })).toBe("1. Do it");
        expect(planOf({ plan: 3 })).toBe("");
        expect(planOf(null)).toBe("");
    });
});

describe("views handed over by the tool", () => {
    const call = (name: string, input: unknown): AgentEvent => ({
        type: "tool_use",
        id: "v1",
        name,
        input,
    });
    const result: AgentEvent = {
        type: "tool_result",
        tool_use_id: "v1",
        content: "ok",
        is_error: false,
    };

    it("stand for the call that showed them, however the agent made it", () => {
        const script = {
            code: "const H = `<p>hi</p>`; return tools.swarmed.show_html({ html: H });",
        };
        const items = toItems([
            call("execute", script),
            result,
            { type: "view", html: "<p>hi</p>" },
        ]);
        expect(items.map((item) => item.kind)).toEqual(["view"]);
        // Also when the call cannot be read at all: the view is shown, the call stays a call.
        const opaque = toItems([
            call("execute", { code: "tools.swarmed.show_html({ html: build() })" }),
            result,
            { type: "view", html: "<p>built</p>" },
        ]);
        expect(opaque.map((item) => item.kind)).toEqual(["tool", "view"]);
    });

    it("hide no tool call when one of them is empty, and are not drawn then", () => {
        const items = toItems([
            toolUse("t1", "ls"),
            toolResult("t1", "a.txt"),
            { type: "view", html: "" },
            call("mcp__swarmed__show_html", { html: "" }),
        ]);
        expect(items.map((item) => item.kind)).toEqual(["tool", "tool"]);
    });

    it("are not needed for a conversation recorded before they existed", () => {
        const items = toItems([call("mcp__swarmed__show_html", { html: "<p>old</p>" }), result]);
        expect(items.map((item) => item.kind)).toEqual(["tool"]);
    });
});

describe("a message from another session", () => {
    it("is shown as that, with its sender, and not as the user's own", () => {
        const left: AgentEvent = {
            type: "user_message",
            text: 'Message from session 4 ("Tests"), another agent session of the same user:\n\nAll green.\nTwo files changed.',
        };
        const own: AgentEvent = { type: "user_message", text: "Message from session 4 is late" };
        const items = toItems([left, own]);
        expect(items[0]).toMatchObject({
            kind: "peer",
            from: 4,
            title: "Tests",
            text: "All green.\nTwo files changed.",
        });
        expect(items[1]).toMatchObject({ kind: "user" });
    });
});

describe("attached files", () => {
    it("are listed under the message and read back from it", () => {
        const files = ["/Users/me/a b.png", "/Users/me/notes.md"];
        const message = withFiles("look at these", files);
        expect(message).toBe(
            "look at these\n\nAttached files:\n- /Users/me/a b.png\n- /Users/me/notes.md",
        );
        expect(splitFiles(message)).toEqual({ text: "look at these", files });
        // Files alone are a message too.
        expect(splitFiles(withFiles("", files))).toEqual({ text: "", files });
        expect(withFiles(" plain ", [])).toBe("plain");
    });

    it("are not read into a message that merely mentions them", () => {
        const prose = "Attached files:\n- one\nand then more words";
        expect(splitFiles(prose)).toEqual({ text: prose, files: [] });
        const inline = "see\nAttached files:\n- /x";
        expect(splitFiles(inline)).toEqual({ text: inline, files: [] });
    });

    it("show on the user\u2019s message in the transcript, by name", () => {
        const [item] = toItems([
            { type: "user_message", text: withFiles("hi", ["/Users/me/a.png"]) },
        ]);
        expect(item).toMatchObject({ kind: "user", text: "hi", files: ["/Users/me/a.png"] });
        expect(fileName("/Users/me/a.png")).toBe("a.png");
        expect(fileName("/Users/me/folder/")).toBe("folder");
    });
});

describe("messageNumber", () => {
    const said = (text: string): AgentEvent => ({ type: "user_message", text });
    const answered = (text: string): AgentEvent => ({ type: "text", text });

    it("counts the messages before an event, whatever else stands between them", () => {
        const events: AgentEvent[] = [
            said("one"),
            answered("first answer"),
            // Only ever shown: the core's list of this session does not hold it.
            { type: "queue", messages: [] },
            { type: "queue", messages: [] },
            said("two"),
            answered("second answer"),
            said("three"),
        ];
        expect(messageNumber(events, 0)).toBe(0);
        expect(messageNumber(events, 4)).toBe(1);
        expect(messageNumber(events, 6)).toBe(2);
    });
});

describe("sameToolCall", () => {
    const input = { command: "ls" };
    const done = (content: string, isError = false) => ({
        name: "Bash",
        input,
        result: { content, isError },
    });

    it("takes two results that hold the same for one, though they are two objects", () => {
        expect(sameToolCall(done("a.txt"), done("a.txt"))).toBe(true);
        const events = [toolUse("t1", "ls"), toolResult("t1", "a.txt")];
        const [first, second] = [toItems(events)[0], toItems(events)[0]];
        expect(
            first?.kind === "tool" && second?.kind === "tool" && sameToolCall(first, second),
        ).toBe(true);
    });

    it("tells apart a call that ended, ended otherwise, or has other arguments", () => {
        const running = { name: "Bash", input, result: null };
        expect(sameToolCall(running, running)).toBe(true);
        expect(sameToolCall(running, done("a.txt"))).toBe(false);
        expect(sameToolCall(done("a.txt"), done("b.txt"))).toBe(false);
        expect(sameToolCall(done("a.txt"), done("a.txt", true))).toBe(false);
        expect(sameToolCall(done("a.txt"), { ...done("a.txt"), input: { command: "ls" } })).toBe(
            false,
        );
        expect(sameToolCall(done("a.txt"), { ...done("a.txt"), name: "Read" })).toBe(false);
    });
});

describe("the agent's own commands", () => {
    const question = {
        question: "Which model should this session run on?",
        header: "Model",
        options: [
            { label: "Default", description: "" },
            { label: "opus", description: "" },
        ],
        multiSelect: false,
    };
    const asked: AgentEvent = {
        type: "command_asked",
        request_id: "model-1",
        command: "model",
        questions: [question],
    };

    it("shows a screen where it was asked for, with what the agent said is on it", () => {
        const sections = [{ title: "Plan limits", rows: [{ label: "Session", value: "40%" }] }];
        const screen: AgentEvent = {
            type: "screen",
            command: "usage",
            title: "Usage",
            note: "On the team plan.",
            sections,
        };
        const [item] = toItems([screen]);
        expect(item).toEqual({
            kind: "screen",
            key: 0,
            command: "usage",
            title: "Usage",
            note: "On the team plan.",
            sections,
        });
        // The parts are the event's own, so a screen drawn once is not drawn again.
        expect(item?.kind === "screen" && item.sections).toBe(sections);
    });

    it("puts a command's question where it stands until it is answered, then says what came of it", () => {
        const [open] = toItems([asked]);
        expect(open).toMatchObject({ kind: "asked", requestId: "model-1", said: null });
        expect(open?.kind === "asked" && open.questions[0]?.options[1]?.label).toBe("opus");
        const answered: AgentEvent = {
            type: "command_answered",
            request_id: "model-1",
            said: "This session's model is now opus.",
        };
        const items = toItems([asked, answered]);
        expect(items).toHaveLength(1);
        expect(items[0]).toMatchObject({
            kind: "asked",
            command: "model",
            said: "This session's model is now opus.",
        });
    });

    it("is not closed by a message or a turn, as an agent's request is", () => {
        const turn: AgentEvent[] = [
            { type: "user_message", text: "carry on" },
            { type: "turn_done", is_error: false, result: null, cost_usd: null },
        ];
        const items = toItems([asked, ...turn]);
        expect(items.find((item) => item.kind === "asked")).toMatchObject({ said: null });
    });

    it("says a setting named with its command in a line of its own", () => {
        const set: AgentEvent = {
            type: "command_answered",
            request_id: "output-style-17",
            said: "Done.",
        };
        expect(toItems([set])).toEqual([
            {
                kind: "asked",
                key: 0,
                requestId: "output-style-17",
                command: "output-style",
                questions: [],
                said: "Done.",
            },
        ]);
    });

    it("draws nothing for questions with nothing to choose from", () => {
        const empty: AgentEvent = { ...asked, questions: [{ ...question, options: [] }] };
        expect(toItems([empty])).toEqual([]);
    });

    it("says a command only works in the agent's terminal, not as a reply of the agent's", () => {
        const refused: AgentEvent = { type: "terminal_only", command: "theme" };
        expect(toItems([refused])).toEqual([{ kind: "terminal-only", key: 0, command: "theme" }]);
    });
});

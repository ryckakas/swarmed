import { describe, expect, it } from "vitest";
import type { AgentEvent, RemoteState } from "./api/types";
import {
    activity,
    continueOffer,
    fileName,
    type LimitItem,
    latestFailure,
    latestLimit,
    latestMerge,
    latestMove,
    mayGoToBackground,
    messageNumber,
    movedSaid,
    planOf,
    sameToolCall,
    shownLines,
    shownTools,
    splitFiles,
    summarize,
    toItems,
    toolRunSummary,
    withFiles,
} from "./transcript";
import { wordsFor } from "./wording";

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
            { kind: "limit", text: "Usage limit reached.", resetsAt: null },
            {
                kind: "notice",
                tone: "error",
                text: "The agent stopped with exit code 1.\nno such model",
            },
        ]);
    });

    it("says no usage report but a turn a limit refused, once, where it stopped", () => {
        const report = (status: string, raw: Record<string, unknown>): AgentEvent => ({
            type: "rate_limit",
            status,
            resets_at: null,
            raw,
        });
        // What session 95 heard on 24 Nov, two windows in turn, with every turn.
        const warning = report("allowed_warning", {
            rateLimitType: "seven_day",
            utilization: 0.99,
        });
        const billed = report("rejected", {
            rateLimitType: "seven_day_overage_included",
            isUsingOverage: true,
        });
        const done: AgentEvent = { type: "turn_done", is_error: false, result: null, cost_usd: 1 };
        expect(kinds([warning, billed, done, warning, billed, done])).toEqual(["turn", "turn"]);
        // Refused, with nothing paying: the turn stops there, and says why, once.
        const refused = report("rejected", { rateLimitType: "five_hour" });
        const failed: AgentEvent = { type: "turn_done", is_error: true, result: null, cost_usd: 1 };
        const said = toItems([warning, refused, refused, failed, refused]).map((item) =>
            item.kind === "limit" ? item.text : item.kind,
        );
        expect(said).toEqual([
            "Usage limit reached (5-hour).",
            "turn",
            "Usage limit reached (5-hour).",
        ]);
    });

    it("keeps on a refused turn's line when every limit that refused it resets", () => {
        const refused = (window: string, resetsAt: number): AgentEvent => ({
            type: "rate_limit",
            status: "rejected",
            resets_at: resetsAt,
            raw: { rateLimitType: window },
        });
        const items = toItems([refused("five_hour", 2_000), refused("seven_day", 9_000)]);
        expect(items).toMatchObject([
            { kind: "limit", text: expect.stringContaining("(5-hour)"), resetsAt: 9_000 },
        ]);
    });

    it("marks the message Swarmed sent by itself when a limit reset", () => {
        const items = toItems([
            { type: "user_message", text: "Continue where you stopped.", at_reset: true },
            { type: "user_message", text: "typed" },
        ]);
        expect(items).toMatchObject([
            { kind: "user", atReset: true },
            { kind: "user", atReset: false },
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
        id: "t1",
        name: "Bash",
        input,
        result: { content, isError },
        moved: false,
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
        const running = { id: "t1", name: "Bash", input, result: null, moved: false };
        expect(sameToolCall(running, running)).toBe(true);
        expect(sameToolCall(running, done("a.txt"))).toBe(false);
        expect(sameToolCall(done("a.txt"), done("b.txt"))).toBe(false);
        expect(sameToolCall(done("a.txt"), done("a.txt", true))).toBe(false);
        expect(sameToolCall(done("a.txt"), { ...done("a.txt"), input: { command: "ls" } })).toBe(
            false,
        );
        expect(sameToolCall(done("a.txt"), { ...done("a.txt"), name: "Read" })).toBe(false);
    });

    it("tells apart another call, and one whose work was moved to the background since", () => {
        expect(sameToolCall(done("a.txt"), { ...done("a.txt"), id: "t2" })).toBe(false);
        expect(sameToolCall(done("a.txt"), { ...done("a.txt"), moved: true })).toBe(false);
    });
});

describe("work sent to the background", () => {
    const moved = (id: string, call: string, wasMoved: boolean): AgentEvent => ({
        type: "background",
        task_id: id,
        description: "",
        kind: "local_bash",
        state: "running",
        summary: null,
        tool_use_id: call,
        output_file: null,
        ...(wasMoved ? { moved: true } : {}),
    });

    it("marks the call whose work was moved there, and no other", () => {
        const events = [
            toolUse("t1", "npm run build"),
            toolUse("t2", "npm run dev"),
            moved("b1", "t1", true),
            toolResult("t1", "Command was manually backgrounded by user with ID: b1."),
            // One that started there was never waited on: nothing was moved.
            moved("b2", "t2", false),
        ];
        const calls = toItems(events).flatMap((item) => (item.kind === "tool" ? [item] : []));
        expect(calls.map((call) => [call.id, call.moved])).toEqual([
            ["t1", true],
            ["t2", false],
        ]);
    });

    it("is offered for a command or sub-agent a call still waits on", () => {
        const call = (name: string, input: unknown, ended = false) => ({
            id: "t1",
            name,
            input,
            result: ended ? { content: "", isError: false } : null,
            moved: false,
        });
        expect(mayGoToBackground(call("Bash", { command: "npm test" }))).toBe(true);
        expect(mayGoToBackground(call("Agent", { prompt: "Look" }))).toBe(true);
        expect(mayGoToBackground(call("Task", { prompt: "Look" }))).toBe(true);
        // Ended, started in the background, or no work of that kind: nothing to send.
        expect(mayGoToBackground(call("Bash", { command: "ls" }, true))).toBe(false);
        const there = { command: "npm run dev", run_in_background: true };
        expect(mayGoToBackground(call("Bash", there))).toBe(false);
        expect(mayGoToBackground(call("Read", { file_path: "a.txt" }))).toBe(false);
        expect(mayGoToBackground(call("Bash", null))).toBe(true);
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

describe("a worktree that moved to another branch", () => {
    const words = wordsFor("plain");
    const moved = (from: string | null, to: string | null, betweenTurns: boolean): AgentEvent => ({
        type: "worktree_moved",
        from,
        to,
        between_turns: betweenTurns,
    });

    it("is an item of its own, in the conversation where it was seen", () => {
        expect(toItems([moved("you/one", "second", false)], words)).toEqual([
            { kind: "moved", key: 0, from: "you/one", to: "second", betweenTurns: false },
        ]);
    });

    it("says what follows when the agent moved it", () => {
        expect(movedSaid("you/one", "second", false, words)).toBe(
            "The worktree moved from you/one to second during this turn: the task's changes and pull request are second's now, and commit and push go to it.",
        );
        expect(movedSaid("you/one", null, false, words)).toBe(
            "The worktree moved from you/one to no branch during this turn, so there is nothing to push until it is on a branch again.",
        );
    });

    it("has one latest move, which is the one that can offer the way back", () => {
        const said: AgentEvent = { type: "text", text: "done" };
        const events = [moved("you/one", "second", false), said, moved("second", "third", false)];
        expect(latestMove(toItems(events, words))).toBe(2);
        expect(latestMove(toItems([said], words))).toBeNull();
    });

    it("says the agent was told when it moved between turns", () => {
        expect(movedSaid("second", "you/one", true, words)).toBe(
            "The worktree was moved from second to you/one since the last turn, and the agent was told with this message.",
        );
    });
});

describe("a change of the pull request", () => {
    const words = wordsFor("plain");
    const changed = (change: Extract<AgentEvent, { type: "pull_changed" }>["change"]) =>
        ({
            type: "pull_changed",
            number: 412,
            url: "https://github.com/o/r/pull/412",
            change,
        }) as AgentEvent;

    it("is one line, and only the latest failure offers to send it", () => {
        const failed = changed({ kind: "checks_failed", checks: ["Unit tests"] });
        const events = [failed, changed({ kind: "commented", by: ["alice"] }), failed];
        const items = toItems(events, words);
        expect(items.map((item) => item.kind)).toEqual(["pull", "pull", "pull"]);
        expect(latestFailure(items)).toBe(2);
        expect(latestFailure(toItems([changed({ kind: "merged" })], words))).toBeNull();
    });

    it("offers putting the task away from the latest merge only", () => {
        const merged = changed({ kind: "merged" });
        const items = toItems([merged, changed({ kind: "approved", by: ["bob"] }), merged], words);
        expect(latestMerge(items)).toBe(2);
        expect(latestMerge(toItems([changed({ kind: "closed" })], words))).toBeNull();
    });
});

describe("shownLines", () => {
    // Claude Code says what it loaded at the start of every turn.
    const init: AgentEvent = {
        type: "init",
        agent_session_id: "abc",
        model: "claude-opus-5-5",
        permission_mode: "default",
        api_key_source: "none",
        mcp_servers: [],
        skills: 91,
        slash_commands: [],
    };
    const ask = (text: string): AgentEvent => ({ type: "user_message", text });
    const say = (text: string): AgentEvent => ({ type: "text", text });
    const done = (cost: number, isError = false): AgentEvent => ({
        type: "turn_done",
        is_error: isError,
        result: null,
        cost_usd: cost,
    });
    // The turns of the issue: two answers, a sub-agent's report read by itself, and then a
    // turn the agent took by itself that came to nothing at all.
    const events = [
        ask("wait 30 seconds in a sub-agent"),
        init,
        say("Started it."),
        done(1),
        init,
        say("The subagent finished its 30-second wait and replied: done"),
        done(1.04),
        init,
        done(1.04),
    ];
    const lines = (shown: ReturnType<typeof toItems>) =>
        shown.map((item) => (item.kind === "turn" ? `turn ${item.costUsd}` : item.kind));

    it("draws the session line once, at the top, and a turn's end only after something", () => {
        expect(lines(shownLines(toItems(events), true))).toEqual([
            "user",
            "session",
            "text",
            "turn 1",
            "text",
            "turn 1.04",
        ]);
    });

    it("keeps the end of a turn with nothing in it that spent more", () => {
        const spent = [...events.slice(0, 4), init, done(1.2)];
        expect(lines(shownLines(toItems(spent), true))).toEqual([
            "user",
            "session",
            "text",
            "turn 1",
            "turn 1.2",
        ]);
    });

    it("draws no end lines when off, but for a turn that failed or was stopped", () => {
        const failed = [
            ...events,
            ask("again"),
            init,
            say("Trying."),
            done(1.5, true),
            init,
            done(1.5, true),
        ];
        expect(lines(shownLines(toItems(failed), false))).toEqual([
            "user",
            "session",
            "text",
            "text",
            "user",
            "text",
            "turn 1.5",
            "turn 1.5",
        ]);
    });

    it("counts a turn of tool calls as one that showed something, whatever shows of them", () => {
        const tool: AgentEvent = {
            type: "tool_use",
            id: "t1",
            name: "Bash",
            input: { command: "ls" },
        };
        const ran = [ask("list"), init, tool, done(1), init, tool, done(1)];
        const shown = shownTools(shownLines(toItems(ran), true), "hidden");
        expect(lines(shown)).toEqual(["user", "session", "turn 1", "turn 1"]);
    });
});

describe("shownTools", () => {
    const plan: AgentEvent = {
        type: "tool_use",
        id: "p",
        name: "ExitPlanMode",
        input: { plan: "1. Fix it" },
    };
    const turn = [
        { type: "user_message", text: "fix the build" },
        toolUse("a", "ls"),
        toolResult("a", "src"),
        toolUse("b", "cargo build"),
        toolResult("b", "error", true),
        { type: "text", text: "It fails; looking closer." },
        toolUse("c", "cat src/main.rs"),
        plan,
        toolUse("d", "cargo test"),
    ] satisfies AgentEvent[];
    const items = toItems(turn);
    const kinds = (shown: ReturnType<typeof toItems>) => shown.map((item) => item.kind);

    it("leaves every call on its own line when all are shown", () => {
        expect(shownTools(items, "all")).toBe(items);
    });

    it("folds the calls that follow each other into one line, keyed by the first", () => {
        const folded = shownTools(items, "folded");
        expect(kinds(folded)).toEqual(["user", "tools", "text", "tools", "tool", "tools"]);
        const first = folded[1];
        expect(first?.kind === "tools" && first.calls.map((call) => call.key)).toEqual([1, 3]);
        expect(first?.key).toBe(1);
    });

    it("keeps calls that talk to the user, and leaves out the rest when hidden", () => {
        const hidden = shownTools(items, "hidden");
        expect(kinds(hidden)).toEqual(["user", "text", "tool"]);
        expect(hidden[2]).toMatchObject({ name: "ExitPlanMode" });
    });

    it("sums a run up by how many, the last, how many failed, and whether it still runs", () => {
        const folded = shownTools(items, "folded");
        const first = folded[1];
        const last = folded[5];
        if (first?.kind !== "tools" || last?.kind !== "tools") {
            throw new Error("expected folded runs");
        }
        expect(toolRunSummary(first.calls)).toMatchObject({
            count: 2,
            last: { name: "Bash", input: { command: "cargo build" } },
            failed: 1,
            running: false,
        });
        expect(toolRunSummary(last.calls)).toMatchObject({ count: 1, running: true });
    });
});

describe("Remote Control in the transcript", () => {
    const at = "https://claude.ai/code/cse_1";
    const report = (state: RemoteState, url: string | null = at, detail: string | null = null) =>
        ({ type: "remote_control", state, url, remote_id: "cse_1", detail }) as const;

    it("marks what was typed on claude.ai, and says once where the session can be reached", () => {
        const items = toItems([
            report("starting", null),
            report("on", null),
            report("on"),
            { type: "user_message", text: "from my phone", remote: true },
            { type: "text", text: "answered here" },
            // The agent started again and attached to the same session: nothing new to say.
            report("paused"),
            report("starting"),
            report("on"),
            { type: "user_message", text: "typed here" },
            report("off"),
            report("starting"),
            report("failed", null, "sign in again"),
        ]);
        expect(items.map((item) => item.kind)).toEqual([
            "remote",
            "user",
            "text",
            "user",
            "remote",
            "remote",
        ]);
        expect(items[0]).toMatchObject({ state: "on", url: at });
        expect(items[1]).toMatchObject({ text: "from my phone", remote: true });
        expect(items[3]).toMatchObject({ text: "typed here", remote: false });
        expect(items[4]).toMatchObject({ state: "off" });
        expect(items[5]).toMatchObject({ state: "failed", detail: "sign in again" });
    });

    it("says it is on again after it was turned off", () => {
        const kinds = toItems([report("on"), report("off"), report("on")]).map(
            (item) => item.kind === "remote" && item.state,
        );
        expect(kinds).toEqual(["on", "off", "on"]);
    });
});

describe("going on when a usage limit resets", () => {
    const limit = (key: number, resetsAt: number | null): LimitItem => ({
        kind: "limit",
        key,
        text: "Usage limit reached (5-hour).",
        resetsAt,
    });
    const said = (key: number) => ({
        kind: "user" as const,
        key,
        text: "go on",
        files: [],
        sessions: [],
        github: [],
        remote: false,
        atReset: false,
    });
    const peer = { kind: "peer" as const, key: 9, from: 3, title: "", text: "done" };

    it("is offered on the latest refused turn's line, until something is written", () => {
        expect(latestLimit([limit(1, 50), limit(4, 60)])).toBe(4);
        expect(latestLimit([limit(1, 50), said(2)])).toBeNull();
        expect(latestLimit([limit(1, 50), peer])).toBeNull();
        expect(latestLimit([said(2), limit(3, 50)])).toBe(3);
        expect(latestLimit([said(2)])).toBeNull();
    });

    it("offers the reset still to come, and says when it is set", () => {
        const now = 100;
        expect(continueOffer(limit(1, 200), true, null, now)).toEqual({
            state: "offered",
            at: 200,
        });
        expect(continueOffer(limit(1, 200), false, null, now)).toBeNull();
        // A reset that has passed is nothing to wait for; one unknown neither.
        expect(continueOffer(limit(1, 100), true, null, now)).toBeNull();
        expect(continueOffer(limit(1, null), true, null, now)).toBeNull();
        // Set, the time it goes is the session's, which the core has the last word on.
        expect(continueOffer(limit(1, 200), true, 300, now)).toEqual({ state: "set", at: 300 });
        expect(continueOffer(limit(1, 50), true, 300, now)).toEqual({ state: "set", at: 300 });
    });
});

import { describe, expect, it } from "vitest";
import type { AgentEvent } from "./api/types";
import { summarize, toItems } from "./transcript";

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
            { kind: "notice", tone: "warning", text: "Usage limit reached (rejected)." },
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
                    { name: "hero", status: "failed" },
                ],
                skills: 89,
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
            },
        ]);
        expect(item).toMatchObject({ login: "agent", mcp: "2 MCP servers passed to the agent" });
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

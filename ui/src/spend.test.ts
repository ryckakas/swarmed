import { describe, expect, it } from "vitest";
import type { AgentEvent } from "./api/types";
import { sessionSpend, spendBasis } from "./spend";

const init = (source: string): AgentEvent => ({
    type: "init",
    agent_session_id: "s",
    model: "m",
    permission_mode: "default",
    api_key_source: source,
    mcp_servers: [],
    skills: 0,
    slash_commands: [],
});
const done = (cost: number | null): AgentEvent => ({
    type: "turn_done",
    is_error: false,
    result: null,
    cost_usd: cost,
});

describe("sessionSpend", () => {
    it("follows the conversation's running total, which Claude Code says with every turn", () => {
        // Claude Code says `init` before every turn, and keeps the total when resumed.
        const turns = [init("none"), done(0.5), init("none"), done(1.2), init("none"), done(1.2)];
        expect(sessionSpend(turns)).toBeCloseTo(1.2);
    });

    it("counts a total lower than the last whole, as one that started from nothing", () => {
        // After `/clear`, Claude Code counts the new conversation from nothing.
        expect(sessionSpend([done(1), init("none"), done(0), done(0.25)])).toBeCloseTo(1.25);
    });

    it("is unknown when no turn said what it cost", () => {
        expect(sessionSpend([init("agent"), done(null)])).toBeNull();
        expect(sessionSpend([])).toBeNull();
    });
});

describe("spendBasis", () => {
    it("tells a subscription from an API key, as the agent last started", () => {
        expect(spendBasis([init("none")])).toBe("subscription");
        expect(spendBasis([init("none"), init("ANTHROPIC_API_KEY")])).toBe("api");
        expect(spendBasis([init("agent")])).toBe("agent");
        expect(spendBasis([done(1)])).toBeNull();
    });
});

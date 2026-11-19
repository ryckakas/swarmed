import { describe, expect, it } from "vitest";
import { sampleProjects } from "./api/mock";
import type { AgentEvent } from "./api/types";
import {
    contextBreakdown,
    contextUsage,
    formatTokens,
    type StatusSource,
    statusParts,
    withItem,
} from "./statusline";

function source(extra: Partial<StatusSource> = {}): StatusSource {
    const task = sampleProjects()[0]?.tasks[0];
    const session = task?.sessions[0];
    if (!(task && session)) {
        throw new Error("the sample data has no session");
    }
    return { session, task, events: [], changes: null, ...extra };
}

const done = (cost: number | null): AgentEvent => ({
    type: "turn_done",
    is_error: false,
    result: null,
    cost_usd: cost,
});

describe("statusParts", () => {
    it("shows the chosen pieces in the chosen order and skips what is not known yet", () => {
        const parts = statusParts(["cost", "changes", "branch"], source());
        expect(parts.map(({ item, text }) => [item, text])).toEqual([
            ["branch", "ricardas/fix-login-redirect"],
        ]);
    });

    it("totals the changes and takes the cost from the latest turn that reported one", () => {
        const parts = statusParts(
            ["changes", "cost"],
            source({
                events: [done(0.1), done(0.31), done(null)],
                changes: [
                    { path: "a", kind: "modified", added: 9, removed: 2 },
                    { path: "b.png", kind: "added", added: null, removed: null },
                ],
            }),
        );
        expect(parts.map((part) => part.text)).toEqual(["2 files +9 −2", "$0.31"]);
        // Additions and removals are runs of their own, so they can be coloured apart.
        expect(parts[0]?.runs).toEqual([
            ["2 files ", "plain"],
            ["+9", "added"],
            [" ", "plain"],
            ["−2", "removed"],
        ]);
    });

    it("prefers the model the agent reported over the one the session was set to", () => {
        const init: AgentEvent = {
            type: "init",
            agent_session_id: "a",
            model: "claude-opus-5-5",
            permission_mode: "auto",
            api_key_source: "none",
            mcp_servers: [],
            skills: 0,
            slash_commands: [],
        };
        expect(statusParts(["model"], source({ events: [init] }))[0]?.runs[0]).toEqual([
            "claude-opus-5-5",
            "plain",
        ]);
    });

    it("shows a chosen effort in the model's badge, after a bar", () => {
        const base = source({});
        const high = {
            ...base,
            session: { ...base.session, model: "opus", effort: "high" as const },
        };
        expect(statusParts(["model"], high)[0]?.runs).toEqual([
            ["opus", "plain"],
            [" | ", "quiet"],
            ["high", "plain"],
        ]);
        // Without a choice it is what the agent reports its default comes to.
        const unset = { ...base, session: { ...base.session, model: "opus", effort: null } };
        expect(statusParts(["model"], unset)[0]?.text).toBe("opus");
        const reported: AgentEvent = { type: "applied", model: "claude-opus-5-5", effort: "xhigh" };
        const told = { ...unset, events: [reported] };
        expect(statusParts(["model"], told)[0]?.text).toBe("claude-opus-5-5 | xhigh");
    });
});

describe("the context window", () => {
    const usage = (used: number): AgentEvent => ({
        type: "context_usage",
        used,
        max: 1_000_000,
        categories: [
            { name: "Free space", tokens: 937_371, kind: "free" },
            { name: "Skills", tokens: 9904, kind: "used" },
            { name: "MCP tools (deferred)", tokens: 129_532, kind: "deferred" },
            { name: "Messages", tokens: 9171, kind: "used" },
            { name: "Nothing", tokens: 0, kind: "used" },
            { name: "Autocompact buffer", tokens: 33_000, kind: "buffer" },
        ],
    });

    it("is shown as a share of the window, and stands out when nearly full", () => {
        const calm = statusParts(["context"], source({ events: [usage(29_629)] }));
        expect(calm[0]?.text).toBe("3% of 1M");
        expect(calm[0]?.runs[0]).toEqual(["3%", "plain"]);
        const full = statusParts(["context"], source({ events: [usage(29_629), usage(850_000)] }));
        expect(full[0]?.runs[0]).toEqual(["85%", "removed"]);
    });

    it("is left out until the agent has reported it", () => {
        expect(statusParts(["context"], source())).toEqual([]);
    });

    it("lists what is used first, largest first, then the buffer, the free space, the rest", () => {
        const latest = contextUsage([usage(29_629)]);
        expect(latest && contextBreakdown(latest).map((category) => category.name)).toEqual([
            "Skills",
            "Messages",
            "Autocompact buffer",
            "Free space",
            "MCP tools (deferred)",
        ]);
    });

    it("writes token counts the short way", () => {
        expect([937, 9171, 29_629, 129_532, 1_000_000].map(formatTokens)).toEqual([
            "937",
            "9.2k",
            "29.6k",
            "130k",
            "1M",
        ]);
    });
});

describe("withItem", () => {
    it("adds a piece at the end and removes it again", () => {
        expect(withItem(["branch"], "cost", true)).toEqual(["branch", "cost"]);
        expect(withItem(["branch", "cost"], "branch", false)).toEqual(["cost"]);
        expect(withItem(["branch"], "branch", true)).toEqual(["branch"]);
    });
});

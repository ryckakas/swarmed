import { describe, expect, it } from "vitest";
import { sampleProjects } from "./api/mock";
import type { AgentEvent } from "./api/types";
import { type StatusSource, statusParts, withItem } from "./statusline";

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
        expect(parts).toEqual([{ item: "branch", text: "ricardas/fix-login-redirect" }]);
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
        expect(statusParts(["model"], source({ events: [init] }))[0]?.text).toBe("claude-opus-5-5");
    });
});

describe("withItem", () => {
    it("adds a piece at the end and removes it again", () => {
        expect(withItem(["branch"], "cost", true)).toEqual(["branch", "cost"]);
        expect(withItem(["branch", "cost"], "branch", false)).toEqual(["cost"]);
        expect(withItem(["branch"], "branch", true)).toEqual(["branch"]);
    });
});

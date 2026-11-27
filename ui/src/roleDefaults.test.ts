import { describe, expect, it } from "vitest";
import type { AgentInfo } from "./api/types";
import { roleModelChoices, withRoleDefault } from "./roleDefaults";

const agent = (id: string, models: string[]): AgentInfo => ({
    id,
    name: id,
    models,
    default_model: null,
    hears_mid_turn: false,
    sends_to_background: false,
});

describe("roleModelChoices", () => {
    it("offers every agent's models once, after leaving it to the agent", () => {
        const agents = [agent("claude", ["opus", "fable"]), agent("codex", ["gpt-5.5", "opus"])];
        expect(roleModelChoices(agents, null)).toEqual([
            ["", "Agent's model"],
            ["opus", "Opus"],
            ["fable", "Fable"],
            ["gpt-5.5", "gpt-5.5"],
        ]);
    });

    it("keeps showing a model chosen before that no agent offers now", () => {
        const choices = roleModelChoices([agent("claude", ["opus"])], "gone-model");
        expect(choices.map(([value]) => value)).toEqual(["", "opus", "gone-model"]);
    });
});

describe("withRoleDefault", () => {
    it("sets a role's model or effort and keeps the other and the other roles", () => {
        const kept = { planner: { effort: "high" as const } };
        const model = withRoleDefault(kept, "reviewer", { model: "fable" });
        expect(model).toEqual({ planner: { effort: "high" }, reviewer: { model: "fable" } });
        const both = withRoleDefault(model, "reviewer", { effort: "medium" });
        expect(both.reviewer).toEqual({ model: "fable", effort: "medium" });
    });

    it("takes a role out once neither is chosen", () => {
        const kept = { reviewer: { model: "fable" } };
        expect(withRoleDefault(kept, "reviewer", { model: "" })).toEqual({});
        expect(withRoleDefault(undefined, "reviewer", { effort: "" })).toEqual({});
    });
});

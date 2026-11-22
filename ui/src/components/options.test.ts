import { describe, expect, it } from "vitest";
import type { AgentInfo } from "../api/types";
import { modelChoices } from "./options";

const opencode: AgentInfo = {
    id: "opencode",
    name: "OpenCode",
    models: ["openrouter/openai/gpt-6.2-sol", "openrouter/x-ai/grok-4.7"],
    default_model: "openrouter/openai/gpt-6.2-sol",
};

describe("modelChoices", () => {
    it("names the model a session left on the default runs on, where Swarmed chooses it", () => {
        expect(modelChoices(opencode)[0]).toEqual([
            "",
            "Default model (openrouter/openai/gpt-6.2-sol)",
        ]);
    });

    it("leaves the default unnamed where the agent chooses it", () => {
        const codex: AgentInfo = { ...opencode, id: "codex", default_model: null };
        expect(modelChoices(codex)[0]).toEqual(["", "Default model"]);
        expect(modelChoices(undefined)).toEqual([["", "Default model"]]);
    });
});

import { describe, expect, it } from "vitest";
import type { AgentInfo } from "../api/types";
import { modelChoices, WORKER_CHOICES } from "./options";

const opencode: AgentInfo = {
    id: "opencode",
    name: "OpenCode",
    models: ["openrouter/openai/gpt-6.2-sol", "openrouter/x-ai/grok-4.7"],
    default_model: "openrouter/openai/gpt-6.2-sol",
    hears_mid_turn: false,
    sends_to_background: false,
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

describe("WORKER_CHOICES", () => {
    it("offers 1 to 20 sessions working for one, as the core takes them, and names six", () => {
        expect(WORKER_CHOICES).toHaveLength(20);
        expect(WORKER_CHOICES[0]).toEqual(["1", "1"]);
        expect(WORKER_CHOICES[5]).toEqual(["6", "6 (default)"]);
        expect(WORKER_CHOICES[19]).toEqual(["20", "20"]);
    });
});

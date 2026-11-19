import { describe, expect, it } from "vitest";
import { iconForAgent, iconForKey } from "./providerIcons";

describe("provider marks", () => {
    it("stand for the provider behind each agent", () => {
        expect(iconForAgent("claude").name).toBe("Claude");
        expect(iconForAgent("codex").name).toBe("OpenAI");
        expect(iconForAgent("opencode").name).toBe("OpenCode");
        // An agent the user set up by hand still gets a mark, a plain one.
        expect(iconForAgent("goose").name).toBe("Agent");
        expect(iconForAgent("goose").path).not.toBe("");
    });

    it("stand for the provider each API key is for", () => {
        expect(iconForKey("OPENROUTER_API_KEY").name).toBe("OpenRouter");
        expect(iconForKey("ANTHROPIC_API_KEY").name).toBe("Anthropic");
        expect(iconForKey("GEMINI_API_KEY").name).toBe("Google Gemini");
        expect(iconForKey("SOMETHING_ELSE").name).toBe("Agent");
    });
});

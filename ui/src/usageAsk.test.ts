import { describe, expect, it } from "vitest";
import { askedLine, usageFigures } from "./usageAsk";

describe("askedLine", () => {
    it("says the figures are being asked for, when they came, or why they did not", () => {
        expect(askedLine({ state: "asking" })).toBe("Asking Claude Code…");
        const at = new Date(2026, 10, 21, 18, 5);
        const time = at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
        expect(askedLine({ state: "asked", at })).toBe(`As Claude Code gave them at ${time}.`);
        expect(
            askedLine({
                state: "failed",
                error: "Claude Code gave no usage figures: it gives them when signed in to a Claude plan",
            }),
        ).toBe(
            "Claude Code gave no usage figures: it gives them when signed in to a Claude plan. These are from the session's last turn.",
        );
        // An error that ends its own sentence does not get a second full stop.
        expect(askedLine({ state: "failed", error: "No answer." })).toBe(
            "No answer. These are from the session's last turn.",
        );
    });
});

describe("usageFigures", () => {
    it("counts what answers asking, and nothing else", () => {
        expect(
            usageFigures([
                { type: "text", text: "hi" },
                { type: "usage_limits", limits: [] },
                {
                    type: "extra_usage",
                    enabled: true,
                    used: 1,
                    limit: null,
                    currency: "USD",
                    decimals: 2,
                    limit_reached: false,
                },
            ]),
        ).toBe(2);
        expect(usageFigures([])).toBe(0);
    });
});

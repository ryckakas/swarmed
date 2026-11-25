import { describe, expect, it } from "vitest";
import { answerHeard, answerWait, askedLine, figuresKey, underWay, usageFigures } from "./usageAsk";

describe("askedLine", () => {
    const second = (d: Date) =>
        d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
    const now = new Date(2026, 10, 25, 16, 33, 23);

    it("says the figures are being asked for, when they came, or why they did not", () => {
        expect(askedLine({ state: "asking" }, now, false)).toBe("Asking Claude Code…");
        // To the second, so that asking again within the minute still shows it came, and
        // whether it changed anything.
        const at = new Date(2026, 10, 21, 18, 5, 42);
        expect(askedLine({ state: "asked", at }, now, false)).toBe(
            `Claude Code gave new figures at ${second(at)}.`,
        );
        expect(askedLine({ state: "asked", at }, now, true)).toBe(
            `Claude Code gave the same figures at ${second(at)}.`,
        );
        expect(
            askedLine(
                {
                    state: "failed",
                    error: "Claude Code gave no usage figures: it gives them when signed in to a Claude plan",
                },
                now,
                false,
            ),
        ).toBe(
            "Claude Code gave no usage figures: it gives them when signed in to a Claude plan. These are the latest any session heard.",
        );
        // An error that ends its own sentence does not get a second full stop.
        expect(askedLine({ state: "failed", error: "No answer." }, now, false)).toBe(
            "No answer. These are the latest any session heard.",
        );
    });

    it("counts down while the question is held back for Claude Code's copy", () => {
        const copyFrom = new Date(2026, 10, 25, 16, 32, 34);
        const goesAt = new Date(2026, 10, 25, 16, 33, 35);
        const waiting = { state: "waiting", goesAt, copyFrom } as const;
        expect(askedLine(waiting, now, false)).toBe(
            `Claude Code reuses the figures it fetched at ${second(copyFrom)} for a minute: asking it in 12 s…`,
        );
        // Part of a second left is a second.
        expect(askedLine(waiting, new Date(goesAt.getTime() - 300), false)).toContain(
            "asking it in 1 s…",
        );
        // Once it went, it is being asked.
        expect(askedLine(waiting, goesAt, false)).toBe("Asking Claude Code…");
    });
});

describe("figuresKey", () => {
    const window = {
        kind: "seven_day",
        name: "weekly",
        used: 0.99,
        resets_at: 1_795_813_200,
        status: "allowed_warning",
        heard_at: 100,
        asked: false,
    };
    const extra = {
        enabled: true,
        used: 15_314,
        limit: 40_000,
        currency: "EUR",
        decimals: 2,
        limit_reached: false,
        heard_at: 100,
    };
    const usage = { windows: [window], extra, billed: null, pays: null };

    it("tells figures that changed from the same figures heard again", () => {
        const again = {
            ...usage,
            windows: [{ ...window, heard_at: 200, asked: true }],
            extra: { ...extra, heard_at: 200 },
        };
        expect(figuresKey(again)).toBe(figuresKey(usage));
        // claude.ai gives the same reset a moment either side of it at each fetch.
        const jittered = { ...usage, windows: [{ ...window, resets_at: 1_795_813_199 }] };
        expect(figuresKey(jittered)).toBe(figuresKey(usage));
        const later = { ...usage, windows: [{ ...window, resets_at: 1_795_813_200 + 1_800 }] };
        expect(figuresKey(later)).not.toBe(figuresKey(usage));
        const reset = { ...usage, windows: [{ ...window, used: 0, status: "allowed" }] };
        expect(figuresKey(reset)).not.toBe(figuresKey(usage));
        const spent = { ...usage, extra: { ...extra, used: 15_400 } };
        expect(figuresKey(spent)).not.toBe(figuresKey(usage));
        expect(figuresKey(null)).toBe(
            figuresKey({ windows: [], extra: null, billed: null, pays: null }),
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

describe("asking held back for Claude Code's copy", () => {
    const goesAt = new Date(1_795_000_061_000);
    const waiting = { state: "waiting", goesAt, copyFrom: new Date(1_795_000_000_000) } as const;

    it("is under way while asked or held back, so it is not asked twice", () => {
        expect(underWay({ state: "asking" })).toBe(true);
        expect(underWay(waiting)).toBe(true);
        expect(underWay({ state: "asked", at: goesAt })).toBe(false);
        expect(underWay({ state: "failed", error: "No answer." })).toBe(false);
        expect(underWay(null)).toBe(false);
    });

    it("takes a figure heard before the question went for something else", () => {
        const before = goesAt.getTime() - 5_000;
        const after = goesAt.getTime() + 500;
        expect(answerHeard(waiting, 2, 2, after)).toBe("none");
        expect(answerHeard(waiting, 2, 3, before)).toBe("early");
        expect(answerHeard(waiting, 2, 3, after)).toBe("answer");
        // A question that went at once is answered by the next figure.
        expect(answerHeard({ state: "asking" }, 2, 3, before)).toBe("answer");
    });

    it("waits for the answer from when the question goes", () => {
        const now = goesAt.getTime() - 40_000;
        expect(answerWait(waiting, now, 30_000)).toBe(70_000);
        expect(answerWait({ state: "asking" }, now, 30_000)).toBe(30_000);
        // Past the time it went, only the usual wait is left.
        expect(answerWait(waiting, goesAt.getTime() + 1_000, 30_000)).toBe(30_000);
    });
});

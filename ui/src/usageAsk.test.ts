import { describe, expect, it } from "vitest";
import {
    answerHeard,
    answerWait,
    askedDetail,
    askedLine,
    figuresKey,
    underWay,
    usageFigures,
    waitedShare,
} from "./usageAsk";

describe("askedLine", () => {
    const second = (d: Date) =>
        d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
    const now = new Date(2026, 10, 25, 16, 33, 23);

    it("says the figures are being refreshed, when they came, or why they did not", () => {
        expect(askedLine({ state: "asking" }, now, false)).toBe("Refreshing now…");
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
        expect(askedLine(waiting, now, false)).toBe("Refreshing in 12 s");
        // Part of a second left is a second.
        expect(askedLine(waiting, new Date(goesAt.getTime() - 300), false)).toBe(
            "Refreshing in 1 s",
        );
        // Once it went, it is being asked.
        expect(askedLine(waiting, goesAt, false)).toBe("Refreshing now…");
    });
});

describe("askedDetail", () => {
    const second = (d: Date) =>
        d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
    const now = new Date(2026, 10, 25, 16, 33, 23);
    const copyFrom = new Date(2026, 10, 25, 16, 32, 34);
    const goesAt = new Date(2026, 10, 25, 16, 33, 35);
    const waiting = { state: "waiting", goesAt, copyFrom } as const;

    it("says why the overview refreshes, and that the figures shown are the latest heard", () => {
        expect(askedDetail(waiting, now)).toBe(
            `Claude Code reuses the figures it fetched at ${second(copyFrom)} for a minute. Until it fetches them again, these are the latest heard.`,
        );
        const asking =
            "Asking Claude Code for the figures as they are now. Until it answers, these are the latest heard.";
        expect(askedDetail({ state: "asking" }, now)).toBe(asking);
        // Once the question went, it says what is under way now.
        expect(askedDetail(waiting, goesAt)).toBe(asking);
    });

    it("says nothing more once the overview is not refreshing", () => {
        expect(askedDetail({ state: "asked", at: now }, now)).toBeNull();
        expect(askedDetail({ state: "failed", error: "No answer." }, now)).toBeNull();
    });
});

describe("waitedShare", () => {
    const copyFrom = new Date(2026, 10, 25, 16, 32, 0);
    const goesAt = new Date(2026, 10, 25, 16, 33, 1);
    const waiting = { state: "waiting", goesAt, copyFrom } as const;
    const at = (ms: number) => new Date(copyFrom.getTime() + ms);

    it("measures the wait from when Claude Code fetched its copy to when the question goes", () => {
        expect(waitedShare(waiting, at(0))).toBe(0);
        expect(waitedShare(waiting, at(30_500))).toBeCloseTo(0.5);
        expect(waitedShare(waiting, at(60_000))).toBeCloseTo(60 / 61);
        // A clock read before the copy's time counts as none of it passed.
        expect(waitedShare(waiting, at(-5_000))).toBe(0);
    });

    it("has no measure while the question is asked, or once it is over", () => {
        // Gone: the answer is under way, and how long it takes is not known.
        expect(waitedShare(waiting, goesAt)).toBeNull();
        expect(waitedShare({ state: "asking" }, goesAt)).toBeNull();
        expect(waitedShare({ state: "asked", at: goesAt }, goesAt)).toBeNull();
        expect(waitedShare({ state: "failed", error: "No answer." }, goesAt)).toBeNull();
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
        const refused = { ...usage, windows: [{ ...window, status: "rejected" }] };
        expect(figuresKey(refused)).not.toBe(figuresKey(usage));
        expect(figuresKey(null)).toBe(
            figuresKey({ windows: [], extra: null, billed: null, pays: null }),
        );
    });

    it("takes a report's fraction and an answer's whole percent of the same usage as the same", () => {
        // Before asking, the window holds a report's figure and word for how near it is; the
        // answer gives the same usage as a whole percent, in words of its own.
        const reported = {
            ...usage,
            windows: [{ ...window, used: 0.4263, status: "allowed_warning" }],
        };
        const answer = { ...window, used: 0.43, status: "warning", heard_at: 200, asked: true };
        const answered = { ...usage, windows: [answer] };
        expect(figuresKey(answered)).toBe(figuresKey(reported));
        // A percent more is a change, as the overview shows it.
        const more = { ...usage, windows: [{ ...answer, used: 0.44 }] };
        expect(figuresKey(more)).not.toBe(figuresKey(reported));
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

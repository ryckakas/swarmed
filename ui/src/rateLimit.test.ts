import { describe, expect, it } from "vitest";
import { isoSeconds, limitNotice, limitWindows, type RateLimit, resetWhen } from "./rateLimit";

// Noon on a Saturday, in the time zone the tests run in.
const now = new Date(2026, 10, 21, 12, 0);
const seconds = (date: Date) => date.getTime() / 1000;

function report(status: string, raw: unknown, resetsAt: Date | null = null): RateLimit {
    return {
        type: "rate_limit",
        status,
        resets_at: resetsAt === null ? null : seconds(resetsAt),
        raw,
    };
}

// What Claude Code sent with the warning that read as a limit reached: every window allowed.
const weeklyWarning = report(
    "allowed_warning",
    {
        status: "allowed_warning",
        rateLimitType: "seven_day",
        utilization: 0.29,
        unifiedWindows: {
            five_hour: { status: "allowed", utilization: 0.31 },
            seven_day: { status: "allowed", utilization: 0.29 },
        },
    },
    new Date(2026, 10, 27, 22, 0),
);

describe("limitNotice", () => {
    it("says nothing while all is well", () => {
        expect(limitNotice(report("allowed", {}), null, now)).toBeNull();
    });

    it("says a warning is a warning, with how much is used and the day it resets", () => {
        const text = limitNotice(weeklyWarning, null, now) ?? "";
        expect(text).toMatch(
            /^Claude Code warns that the weekly limit is getting closer: 29% used\./,
        );
        expect(text).toContain("Nothing is limited yet.");
        expect(text).not.toContain("reached");
        // Six days away: the time alone would read as tonight.
        expect(text).toContain(
            `It resets ${resetWhen(seconds(new Date(2026, 10, 27, 22, 0)), now)}.`,
        );
        expect(text).toMatch(/ on .+ at /);
    });

    it("says a warning once, not again with every turn", () => {
        expect(limitNotice(weeklyWarning, weeklyWarning, now)).toBeNull();
        // Once all was well again, a new warning is news.
        expect(limitNotice(weeklyWarning, report("allowed", {}), now)).not.toBeNull();
        // A warning about another window is too.
        const hourly = report("allowed_warning", { rateLimitType: "five_hour" });
        expect(limitNotice(hourly, weeklyWarning, now)).toMatch(/the 5-hour limit/);
    });

    it("says every time a limit is reached, and which", () => {
        const reached = report(
            "rejected",
            { rateLimitType: "five_hour" },
            new Date(2026, 10, 21, 17, 0),
        );
        const text = `Usage limit reached (5-hour). It resets ${resetWhen(seconds(new Date(2026, 10, 21, 17, 0)), now)}.`;
        expect(limitNotice(reached, null, now)).toBe(text);
        expect(limitNotice(reached, reached, now)).toBe(text);
        expect(limitNotice(report("rejected", null), null, now)).toBe("Usage limit reached.");
    });
});

describe("resetWhen", () => {
    it("gives the time alone today, and the day as well on another", () => {
        const today = resetWhen(seconds(new Date(2026, 10, 21, 22, 0)), now);
        expect(today).toMatch(/^at \S/);
        expect(today).not.toContain(" on ");
        const later = resetWhen(seconds(new Date(2026, 10, 27, 22, 0)), now);
        expect(later).toMatch(/^on .*27.* at /);
    });
});

describe("limitWindows", () => {
    it("lists every window of the latest report, named, in Claude Code's order", () => {
        const earlier = report("allowed", {
            unifiedWindows: { five_hour: { status: "allowed", utilization: 0.1 } },
        });
        const latest = report("allowed_warning", {
            unifiedWindows: {
                five_hour: { status: "allowed", utilization: 0.31, resetsAt: 100 },
                seven_day: { status: "allowed", utilization: 0.29, resetsAt: 200 },
                seven_day_overage_included: { status: "allowed", utilization: 0.44 },
                seven_day_new_kind: { status: "rejected", utilization: 1 },
                broken: { status: "allowed" },
            },
        });
        expect(limitWindows([earlier, { type: "text", text: "hi" }, latest])).toEqual([
            { name: "5-hour", used: 0.31, resetsAt: 100, status: "allowed" },
            { name: "weekly", used: 0.29, resetsAt: 200, status: "allowed" },
            {
                name: "weekly, extra usage included",
                used: 0.44,
                resetsAt: null,
                status: "allowed",
            },
            { name: "seven day new kind", used: 1, resetsAt: null, status: "rejected" },
        ]);
    });

    it("is empty without a report that lists windows, as from every agent but Claude Code", () => {
        expect(limitWindows([])).toEqual([]);
        expect(limitWindows([report("allowed", null)])).toEqual([]);
        expect(limitWindows([report("allowed", { unifiedWindows: "?" })])).toEqual([]);
    });
});

describe("a limit reached while extra usage carries on", () => {
    // What Claude Code reported on 19 Nov when the 5-hour window filled and work went on.
    const billed = report("rejected", {
        status: "rejected",
        rateLimitType: "five_hour",
        isUsingOverage: true,
        overageInUse: true,
        overageStatus: "allowed",
    });

    it("says the work goes on, billed, not that it stopped", () => {
        const text = limitNotice(billed, null, now) ?? "";
        expect(text).toBe(
            "The 5-hour limit is reached: Claude Code goes on with extra usage, which is billed.",
        );
    });

    it("says it once, not with every billed turn after it", () => {
        expect(limitNotice(billed, billed, now)).toBeNull();
        // Back within the plan and over again: news once more.
        expect(limitNotice(billed, report("allowed", {}), now)).not.toBeNull();
    });
});

describe("limits Claude Code gave when asked", () => {
    const asked = {
        type: "usage_limits" as const,
        limits: [
            {
                kind: "session",
                model: null,
                percent: 36,
                resets_at: "2026-11-21T19:59:59.723480+00:00",
                severity: "normal",
            },
            { kind: "weekly_all", model: null, percent: 30, resets_at: null, severity: "normal" },
            {
                kind: "weekly_scoped",
                model: "Fable",
                percent: 92,
                resets_at: null,
                severity: "warning",
            },
            { kind: "monthly_new", model: null, percent: 5, resets_at: null, severity: "normal" },
        ],
    };
    const reported = report("allowed", {
        unifiedWindows: { five_hour: { status: "allowed", utilization: 0.1 } },
    });

    it("are named as the reports name them, one model's after it", () => {
        expect(limitWindows([asked])).toEqual([
            {
                name: "5-hour",
                used: 0.36,
                resetsAt: Date.UTC(2026, 10, 21, 19, 59, 59, 723) / 1000,
                status: "allowed",
            },
            { name: "weekly", used: 0.3, resetsAt: null, status: "allowed" },
            { name: "weekly Fable", used: 0.92, resetsAt: null, status: "warning" },
            { name: "monthly new", used: 0.05, resetsAt: null, status: "allowed" },
        ]);
    });

    it("stand until a later report, which stands until they are asked for again", () => {
        expect(limitWindows([reported, asked])[0]?.used).toBe(0.36);
        expect(limitWindows([asked, reported])[0]?.used).toBe(0.1);
        // A report that lists no windows does not hide the ones asked for.
        expect(limitWindows([asked, report("allowed", null)])[0]?.used).toBe(0.36);
    });
});

describe("isoSeconds", () => {
    it("reads Claude Code's times, microseconds and all, and nothing else", () => {
        expect(isoSeconds("2026-11-27T20:59:59+00:00")).toBe(
            Date.UTC(2026, 10, 27, 20, 59, 59) / 1000,
        );
        expect(isoSeconds("2026-11-27T20:59:59.723508+00:00")).toBe(
            Date.UTC(2026, 10, 27, 20, 59, 59, 723) / 1000,
        );
        expect(isoSeconds(null)).toBeNull();
        expect(isoSeconds("soon")).toBeNull();
    });
});

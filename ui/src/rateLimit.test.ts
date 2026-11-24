import { describe, expect, it } from "vitest";
import type { AccountUsage, LimitWarnings, UsageWindow } from "./api/types";
import {
    accountWindows,
    asOf,
    type LimitWindow,
    limitLevel,
    limitNotice,
    limitWarning,
    newestHeard,
    type RateLimit,
    resetWhen,
    untilNextReset,
    warningLine,
    windowNote,
    windowNotes,
} from "./rateLimit";

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
        expect(limitNotice(report("allowed", {}), null, now, "frequent")).toBeNull();
    });

    it("says a warning is a warning, with how much is used and the day it resets", () => {
        const text = limitNotice(weeklyWarning, null, now, "frequent") ?? "";
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
        expect(limitNotice(weeklyWarning, weeklyWarning, now, "frequent")).toBeNull();
        // Once all was well again, a new warning is news.
        expect(limitNotice(weeklyWarning, report("allowed", {}), now, "frequent")).not.toBeNull();
        // A warning about another window is too.
        const hourly = report("allowed_warning", { rateLimitType: "five_hour" });
        expect(limitNotice(hourly, weeklyWarning, now, "frequent")).toMatch(/the 5-hour limit/);
    });

    it("says every time a limit is reached, and which", () => {
        const reached = report(
            "rejected",
            { rateLimitType: "five_hour" },
            new Date(2026, 10, 21, 17, 0),
        );
        const text = `Usage limit reached (5-hour). It resets ${resetWhen(seconds(new Date(2026, 10, 21, 17, 0)), now)}.`;
        expect(limitNotice(reached, null, now, "frequent")).toBe(text);
        expect(limitNotice(reached, reached, now, "frequent")).toBe(text);
        expect(limitNotice(report("rejected", null), null, now, "frequent")).toBe(
            "Usage limit reached.",
        );
    });
});

describe("limitNotice, as often as the user wants usage warnings", () => {
    const weekly = (used: number | null, window = "seven_day") =>
        report("allowed_warning", {
            rateLimitType: window,
            ...(used === null ? {} : { utilization: used }),
        });
    // What a weekly window that fills up through a day of work reports, turn by turn.
    const filling = [weekly(0.52), weekly(0.6), weekly(0.76), weekly(0.81), weekly(0.9)];
    const said = (mode: LimitWarnings, reports: RateLimit[]) =>
        reports.flatMap((event, at) => {
            const text = limitNotice(event, reports[at - 1] ?? null, now, mode);
            return text === null ? [] : [text];
        });

    it("says each warning once when frequent, as Swarmed always did", () => {
        const text = said("frequent", filling);
        expect(text).toHaveLength(1);
        expect(text[0]).toMatch(/weekly limit is getting closer: 52% used/);
        expect(said("frequent", [weekly(null)])).toHaveLength(1);
    });

    it("says nothing below 75% when rare, and once when the window passes it", () => {
        const text = said("rare", filling);
        expect(text).toHaveLength(1);
        expect(text[0]).toMatch(/weekly limit is getting closer: 76% used/);
        expect(said("rare", [weekly(0.29), weekly(0.74)])).toEqual([]);
        expect(said("rare", [weekly(0.75)])).toHaveLength(1);
        // A warning that does not say how full its window is says nothing.
        expect(said("rare", [weekly(null)])).toEqual([]);
        expect(said("rare", [weekly(null), weekly(0.8)])).toHaveLength(1);
    });

    it("says it again when rare for another window, or once all was well again", () => {
        const hourly = weekly(0.8, "five_hour");
        const both = said("rare", [weekly(0.8), hourly]);
        expect(both).toHaveLength(2);
        expect(both[1]).toMatch(/the 5-hour limit is getting closer: 80% used/);
        const allowed = report("allowed", {});
        expect(said("rare", [weekly(0.8), allowed, weekly(0.8)])).toHaveLength(2);
    });

    it("says no warning at all when none", () => {
        expect(said("none", [...filling, weekly(null), weekly(0.8, "five_hour")])).toEqual([]);
    });

    it("says every time a limit is reached, and work going on billed, whatever the mode", () => {
        const reached = report("rejected", { rateLimitType: "five_hour" });
        const billed = report("rejected", { rateLimitType: "five_hour", isUsingOverage: true });
        for (const mode of ["frequent", "rare", "none"] as const) {
            expect(said(mode, [reached, reached])).toEqual([
                "Usage limit reached (5-hour).",
                "Usage limit reached (5-hour).",
            ]);
            expect(said(mode, [weekly(0.9), billed])).toContain(
                "The 5-hour limit is reached: Claude Code goes on with extra usage, which is billed.",
            );
        }
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
        const text = limitNotice(billed, null, now, "frequent") ?? "";
        expect(text).toBe(
            "The 5-hour limit is reached: Claude Code goes on with extra usage, which is billed.",
        );
    });

    it("says it once, not with every billed turn after it", () => {
        expect(limitNotice(billed, billed, now, "frequent")).toBeNull();
        // Back within the plan and over again: news once more.
        expect(limitNotice(billed, report("allowed", {}), now, "frequent")).not.toBeNull();
    });
});

describe("the account's windows", () => {
    // Heard at 11:00 after a turn, and asked for at 10:00, Fable's limit with it.
    const heard = seconds(new Date(2026, 10, 21, 11, 0));
    const asked = seconds(new Date(2026, 10, 21, 10, 0));
    const window = (
        kind: string,
        used: number,
        resetsAt: number | null,
        at = heard,
    ): UsageWindow => ({
        kind,
        name: kind === "seven_day_fable" ? "weekly Fable" : kind,
        used,
        resets_at: resetsAt,
        status: used >= 1 ? "critical" : "allowed",
        heard_at: at,
        asked: at === asked,
    });
    const usage = (...windows: UsageWindow[]): AccountUsage => ({
        windows,
        extra: null,
        billed: null,
    });
    const inAnHour = seconds(new Date(2026, 10, 21, 13, 0));
    const anHourAgo = seconds(new Date(2026, 10, 21, 11, 0));

    it("stand as heard until they reset", () => {
        const fable = window("seven_day_fable", 1, inAnHour, asked);
        const [shown] = accountWindows(usage(fable), seconds(now));
        expect(shown).toEqual({
            name: "weekly Fable",
            used: 1,
            resetsAt: inAnHour,
            status: "critical",
            heardAt: asked,
            asked: true,
            reset: false,
        });
        expect(limitWarning(accountWindows(usage(fable), seconds(now)))?.name).toBe("weekly Fable");
    });

    it("count as reset, empty, once their reset time has passed, with nothing heard since", () => {
        const fiveHour = window("five_hour", 0.9, anHourAgo);
        const [shown] = accountWindows(usage(fiveHour), seconds(now));
        expect(shown).toMatchObject({ used: 0, status: "allowed", reset: true });
        expect(limitWarning(accountWindows(usage(fiveHour), seconds(now)))).toBeNull();
        // The same window just before its reset still warns.
        const before = accountWindows(usage(fiveHour), anHourAgo - 1);
        expect(limitWarning(before)?.used).toBe(0.9);
    });

    it("are none before anything is heard, or for another agent's session", () => {
        expect(accountWindows(null, seconds(now))).toEqual([]);
        expect(accountWindows(usage(), seconds(now))).toEqual([]);
    });

    it("are looked at again when the next of them resets, and not before", () => {
        const both = usage(
            window("five_hour", 0.9, inAnHour),
            window("seven_day", 0.5, inAnHour + 86_400),
            window("gone", 0.5, anHourAgo),
            window("unsaid", 0.5, null),
        );
        expect(untilNextReset(both, seconds(now))).toBe(3600 * 1000 + 1000);
        expect(untilNextReset(usage(window("gone", 0.5, anHourAgo)), seconds(now))).toBeNull();
        expect(untilNextReset(null, seconds(now))).toBeNull();
        // The limit extra usage paid past counts too, and a wait too long for a timer is cut.
        const billed = { on: true, resets_at: seconds(now) + 60, heard_at: heard };
        expect(untilNextReset({ ...usage(), billed }, seconds(now))).toBe(61_000);
        const far = usage(window("seven_day", 0.5, seconds(now) + 60 * 86_400));
        expect(untilNextReset(far, seconds(now))).toBe(2 ** 31 - 1);
    });

    it("say when they reset, and how old a figure older than the newest is", () => {
        const fable = window("seven_day_fable", 1, inAnHour, asked);
        const fiveHour = window("five_hour", 0.3, anHourAgo);
        const shown = accountWindows(usage(fable, fiveHour), seconds(now));
        const newest = newestHeard(shown);
        expect(newest).toBe(heard);
        const [fableShown, fiveHourShown] = shown;
        const fableNotes = fableShown && windowNotes(fableShown, newest, now);
        expect(fableNotes?.resets).toMatch(/^Resets at /);
        expect(fableNotes?.age).toBe(`Heard ${resetWhen(asked, now)}`);
        expect(fableNotes && windowNote(fableNotes)).toMatch(/^Resets at .+ · Heard at .+/);
        const fiveHourNotes = fiveHourShown && windowNotes(fiveHourShown, newest, now);
        expect(fiveHourNotes).toEqual({ resets: expect.stringMatching(/^Reset at /), age: null });
        const unsaid = accountWindows(usage(window("five_hour", 0.3, null)), seconds(now));
        const none = unsaid[0] && windowNotes(unsaid[0], heard, now);
        expect(none && windowNote(none)).toBeNull();
        expect(newestHeard([])).toBeNull();
    });

    it("say how old a figure is: the time today, the day as well before", () => {
        expect(asOf(heard, now)).toMatch(/^as of \S/);
        expect(asOf(heard, now)).not.toContain(",");
        expect(asOf(seconds(new Date(2026, 10, 19, 9, 0)), now)).toMatch(/^as of .*19.*, \S/);
    });
});

describe("warning of a limit", () => {
    const window = (name: string, used: number, status = "allowed"): LimitWindow => ({
        name,
        used,
        resetsAt: null,
        status,
        heardAt: 0,
        asked: false,
        reset: false,
    });

    it("starts at 75% as Claude Code's does, by how full a window is and not by its report", () => {
        expect(limitLevel(window("5-hour", 0.74))).toBe("fine");
        expect(limitLevel(window("5-hour", 0.75))).toBe("warning");
        expect(limitLevel(window("5-hour", 1))).toBe("reached");
        expect(limitLevel(window("5-hour", 0.4, "rejected"))).toBe("reached");
        // Claude Code once said `allowed_warning` of a window 29% full.
        expect(limitLevel(window("weekly", 0.29, "allowed_warning"))).toBe("fine");
    });

    it("names the window most worth it: a used-up one first, then the fullest", () => {
        expect(limitWarning([window("5-hour", 0.3), window("weekly", 0.6)])).toBeNull();
        expect(limitWarning([window("5-hour", 0.78), window("weekly", 0.91)])?.name).toBe("weekly");
        expect(
            limitWarning([window("weekly", 0.95), window("5-hour", 0.5, "rejected")])?.name,
        ).toBe("5-hour");
        expect(limitWarning([])).toBeNull();
    });

    it("says what it comes to", () => {
        expect(warningLine(window("weekly Fable", 0.874))).toBe("Weekly Fable: 87% used.");
        expect(warningLine(window("5-hour", 1))).toBe("5-hour: used up.");
    });
});

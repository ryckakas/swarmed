import { describe, expect, it } from "vitest";
import type { AccountUsage, LimitWarnings, UsageWindow } from "./api/types";
import { onExtraUsageNow } from "./extraUsage";
import {
    accountWindows,
    asOf,
    BILLED,
    clockTime,
    type Dismissed,
    dismissalOf,
    heldBack,
    type LimitWindow,
    limitLevel,
    limitWarning,
    lowestSaid,
    newestHeard,
    type RateLimit,
    readDismissed,
    refusalNotice,
    resetWhen,
    STOPPED,
    untilNextReset,
    usageNotice,
    usageNoticeText,
    warningLine,
    windowLevel,
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

describe("refusalNotice", () => {
    it("says a turn a limit refused, which window and when it resets", () => {
        const at = new Date(2026, 10, 21, 17, 0);
        const refused = report("rejected", { rateLimitType: "five_hour" }, at);
        expect(refusalNotice(refused, now)).toBe(
            `Usage limit reached (5-hour). It resets ${resetWhen(seconds(at), now)}.`,
        );
        expect(refusalNotice(report("rejected", null), now)).toBe("Usage limit reached.");
    });

    it("says nothing of a warning, or of a limit extra usage pays past", () => {
        expect(refusalNotice(report("allowed", {}), now)).toBeNull();
        expect(refusalNotice(report("allowed_warning", { utilization: 0.99 }), now)).toBeNull();
        const billed = report("rejected", { rateLimitType: "five_hour", isUsingOverage: true });
        expect(refusalNotice(billed, now)).toBeNull();
        const inUse = report("rejected", { rateLimitType: "five_hour", overageInUse: true });
        expect(refusalNotice(inUse, now)).toBeNull();
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

describe("clockTime", () => {
    it("gives the time alone today, and the day's short name before it on another", () => {
        const today = clockTime(seconds(new Date(2026, 10, 21, 22, 0)), now);
        expect(today).toMatch(/^\d/);
        const later = clockTime(seconds(new Date(2026, 10, 27, 22, 0)), now);
        expect(later).toMatch(/^\D+ \d/);
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
        pays: null,
    });
    const inAnHour = seconds(new Date(2026, 10, 21, 13, 0));
    const anHourAgo = seconds(new Date(2026, 10, 21, 11, 0));

    it("stand as heard until they reset", () => {
        const fable = window("seven_day_fable", 1, inAnHour, asked);
        const [shown] = accountWindows(usage(fable), seconds(now));
        expect(shown).toEqual({
            kind: "seven_day_fable",
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
        kind: name,
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

    it("takes an answer's 100% for rounded, reached only when a report said so", () => {
        // A report after a turn said 99% and `allowed_warning` at the second an answer said 100.
        const answered = { ...window("weekly", 1, "critical"), asked: true };
        expect(limitLevel(answered)).toBe("warning");
        expect(limitLevel({ ...answered, status: "rejected" })).toBe("reached");
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

describe("the usage notice", () => {
    // Friday 27 Nov at 22:00, when the weekly windows reset, and an hour from now.
    const friday = seconds(new Date(2026, 10, 27, 22, 0));
    const inAnHour = seconds(now) + 3600;
    const window = (kind: string, used: number, status = "allowed", resetsAt = friday) => ({
        kind,
        name: kind,
        used,
        resetsAt,
        status,
        heardAt: seconds(now),
        asked: false,
        reset: false,
    });
    const notice = (
        windows: LimitWindow[],
        mode: LimitWarnings = "frequent",
        dismissed: Dismissed | null = null,
        paying = false,
        at = seconds(now),
    ) => usageNotice(windows, paying, mode, dismissed, at);

    it("has a level for each window, by how full it is and what pays past its limit", () => {
        expect(windowLevel(window("five_hour", 0.49), false)).toBe(0);
        expect(windowLevel(window("five_hour", 0.5), false)).toBe(1);
        expect(windowLevel(window("five_hour", 0.75), false)).toBe(2);
        expect(windowLevel(window("five_hour", 0.9), false)).toBe(3);
        expect(windowLevel(window("five_hour", 1), true)).toBe(BILLED);
        expect(windowLevel(window("five_hour", 1), false)).toBe(STOPPED);
        expect(windowLevel(window("five_hour", 0.2, "rejected"), false)).toBe(STOPPED);
        // Claude Code's own word for a window less than a third full is not taken.
        expect(windowLevel(window("seven_day", 0.29, "allowed_warning"), false)).toBe(0);
    });

    it("says the levels the setting picks", () => {
        expect(lowestSaid("frequent")).toBe(1);
        expect(lowestSaid("rare")).toBe(3);
        expect(lowestSaid("none")).toBe(STOPPED);
    });

    it("is the same through reports of two windows in turn: billed, weekly, resets Friday", () => {
        // Session 95 on 24 Nov: a warning of the weekly window and a billed limit of the
        // weekly window with extra usage, in turn, with every turn. The account keeps the
        // newest of each, and the billed limit until it resets.
        const kinds = (billedAt: number, warnedAt: number): AccountUsage => ({
            windows: [
                { ...usageWindow("five_hour", 0.09, "allowed", inAnHour), heard_at: warnedAt },
                {
                    ...usageWindow("seven_day", 0.99, "allowed_warning", friday),
                    heard_at: warnedAt,
                },
                {
                    ...usageWindow("seven_day_overage_included", 1, "rejected", friday),
                    heard_at: billedAt,
                },
            ],
            extra: null,
            billed: { on: true, resets_at: friday, heard_at: billedAt },
            pays: null,
        });
        const after = [kinds(100, 90), kinds(100, 110), kinds(200, 110), kinds(200, 210)];
        const shown = after.map((usage) => {
            const windows = accountWindows(usage, seconds(now));
            const said = notice(windows, "frequent", null, onExtraUsageNow(usage, seconds(now)));
            return (
                said && {
                    kind: said.window.kind,
                    level: said.level,
                    text: usageNoticeText(said, now),
                }
            );
        });
        const billed = {
            kind: "seven_day_overage_included",
            level: BILLED,
            text: `The weekly model limit is reached: Claude Code goes on with extra usage, which is billed. It resets ${resetWhen(friday, now)}.`,
        };
        expect(shown).toEqual([billed, billed, billed, billed]);
        expect(resetWhen(friday, now)).toMatch(/^on .*27.* at /);
    });

    it("shows the highest level reached, and of those the fullest window", () => {
        expect(notice([window("five_hour", 0.3)])).toBeNull();
        expect(notice([window("five_hour", 0.8), window("seven_day", 0.6)])?.window.kind).toBe(
            "five_hour",
        );
        expect(notice([window("five_hour", 0.92), window("seven_day", 0.97)])?.window.kind).toBe(
            "seven_day",
        );
        expect(notice([window("seven_day", 0.97), window("five_hour", 1)])?.level).toBe(STOPPED);
        expect(notice([])).toBeNull();
    });

    it("shows again at each level when frequent, only from 90% when rare, and only stopped when none", () => {
        // A weekly window filling up, with the notice dismissed each time it shows.
        const filling = [0.5, 0.6, 0.75, 0.8, 0.9, 0.95, 1];
        const shown = (mode: LimitWarnings) => {
            let dismissed: Dismissed | null = null;
            return filling.flatMap((used) => {
                const windows = [window("seven_day", used)];
                const said = notice(windows, mode, dismissed);
                if (said === null) {
                    return [];
                }
                dismissed = dismissalOf(windows, false);
                return [said.level];
            });
        };
        expect(shown("frequent")).toEqual([1, 2, 3, STOPPED]);
        expect(shown("rare")).toEqual([3, STOPPED]);
        expect(shown("none")).toEqual([STOPPED]);
    });

    it("stays dismissed until a higher level, or until its window resets and climbs back", () => {
        const fiveHour = (used: number) => window("five_hour", used, "allowed", inAnHour);
        const windows = [fiveHour(0.8), window("seven_day", 0.3)];
        const dismissed = dismissalOf(windows, false);
        expect(dismissed).toEqual({ level: 2, resets: { five_hour: inAnHour, seven_day: friday } });
        expect(notice(windows, "frequent", dismissed)).toBeNull();
        // Another window climbing to no higher a level stays away too.
        expect(notice([fiveHour(0.8), window("seven_day", 0.6)], "frequent", dismissed)).toBeNull();
        // A higher level is news.
        expect(
            notice([fiveHour(0.9), window("seven_day", 0.3)], "frequent", dismissed)?.level,
        ).toBe(3);
        // The 5-hour window resets and climbs back to 50%: news, in an hour and a bit.
        const later = inAnHour + 600;
        const climbed = [fiveHour(0.5), window("seven_day", 0.3)];
        expect(notice(climbed, "frequent", dismissed, false, later)?.window.kind).toBe("five_hour");
        expect(heldBack(window("seven_day", 0.6), 1, dismissed, later)).toBe(true);
        // A window nobody knew of when it was dismissed is news at any level.
        expect(heldBack(window("seven_day_fable", 0.5), 1, dismissed, later)).toBe(false);
        expect(heldBack(window("seven_day_fable", 0.8), 2, dismissed, later)).toBe(false);
    });

    it("shows a window first reported after it was dismissed, even at the level dismissed", () => {
        // Requests stopped by the 5-hour limit, dismissed while it was the only window known.
        const fiveHour = window("five_hour", 1, "rejected", inAnHour);
        const dismissed = dismissalOf([fiveHour], false);
        expect(dismissed).toEqual({ level: STOPPED, resets: { five_hour: inAnHour } });
        expect(notice([fiveHour], "none", dismissed)).toBeNull();
        // The weekly limit is reported later, and stops requests too.
        const weekly = window("seven_day", 1, "rejected");
        expect(notice([fiveHour, weekly], "none", dismissed)?.window.kind).toBe("seven_day");
    });

    it("keeps a window that said no reset away until a higher level", () => {
        // There is no telling when it resets: as news, it would come straight back.
        const unsaid = { ...window("seven_day", 0.8), resetsAt: null };
        const dismissed = dismissalOf([unsaid], false);
        expect(dismissed).toEqual({ level: 2, resets: { seven_day: null } });
        expect(notice([unsaid], "frequent", dismissed, false, friday + 60)).toBeNull();
        expect(notice([{ ...unsaid, used: 0.9 }], "frequent", dismissed)?.level).toBe(3);
    });

    it("is dismissed for the account through a restart, from what this machine kept", () => {
        const dismissed = dismissalOf([window("seven_day", 1)], true);
        expect(readDismissed(JSON.parse(JSON.stringify(dismissed)))).toEqual(dismissed);
        expect(readDismissed(null)).toBeNull();
        expect(readDismissed({ level: 9, resets: {} })).toBeNull();
        expect(readDismissed({ level: 2 })).toBeNull();
        expect(readDismissed({ level: 2, resets: { five_hour: "soon" } })).toEqual({
            level: 2,
            resets: {},
        });
    });

    it("says what it should have said on 25 Nov: all but used up, and billed once reached", () => {
        // #435: the weekly window at 99% and `allowed_warning` after a turn, 100 when asked
        // that second, with extra usage turned on and under its cap.
        const resets = ` It resets ${resetWhen(friday, now)}.`;
        const answered = { ...window("seven_day", 1, "critical"), name: "weekly", asked: true };
        const said = notice([answered], "frequent", null, true);
        expect(said?.level).toBe(3);
        expect(said && usageNoticeText(said, now)).toBe(
            `The weekly limit is all but used up. Nothing is limited yet.${resets}`,
        );
        // Reached, as a report says: billed while extra usage can pay, refused when it cannot.
        const reached = { ...window("seven_day", 1.01, "rejected"), name: "weekly" };
        expect(notice([reached], "frequent", null, true)?.level).toBe(BILLED);
        expect(notice([reached], "frequent", null, false)?.level).toBe(STOPPED);
    });

    it("says a model's own limit stops that model alone", () => {
        const resets = ` It resets ${resetWhen(friday, now)}.`;
        const fable = { ...window("seven_day_fable", 1, "rejected"), name: "weekly Fable" };
        expect(usageNoticeText({ window: fable, level: STOPPED }, now)).toBe(
            `The weekly Fable limit is reached, and nothing pays past it: Claude Code's requests to that model are refused, and other models go on.${resets}`,
        );
    });

    it("says what it comes to at each level", () => {
        const weekly = window("weekly", 0.874);
        const resets = ` It resets ${resetWhen(friday, now)}.`;
        expect(usageNoticeText({ window: weekly, level: 2 }, now)).toBe(
            `The weekly limit is 87% used. Nothing is limited yet.${resets}`,
        );
        expect(usageNoticeText({ window: window("weekly", 1), level: STOPPED }, now)).toBe(
            `The weekly limit is reached, and nothing pays past it: Claude Code's requests are refused.${resets}`,
        );
        const unsaid = { ...window("5-hour", 1), resetsAt: null };
        expect(usageNoticeText({ window: unsaid, level: BILLED }, now)).toBe(
            "The 5-hour limit is reached: Claude Code goes on with extra usage, which is billed.",
        );
    });
});

/** One of the account's windows as the core sends it, named as the core names it. */
function usageWindow(kind: string, used: number, status: string, resetsAt: number): UsageWindow {
    const names: Record<string, string> = {
        five_hour: "5-hour",
        seven_day: "weekly",
        seven_day_overage_included: "weekly model",
    };
    return {
        kind,
        name: names[kind] ?? kind,
        used,
        resets_at: resetsAt,
        status,
        heard_at: 0,
        asked: false,
    };
}

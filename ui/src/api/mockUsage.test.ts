import { describe, expect, it } from "vitest";
import { heardUsage, sampleAccountUsage } from "./mockUsage";
import type { AccountUsage, AgentEvent } from "./types";

// The sample account's usage follows the sample agent's reports as the core follows Claude
// Code's: each window keeps the newest figure any report gave it.

const answer: AgentEvent = {
    type: "usage_limits",
    limits: [
        { kind: "session", model: null, percent: 36, resets_at: null, severity: "normal" },
        {
            kind: "weekly_scoped",
            model: "Fable",
            percent: 100,
            resets_at: "2026-11-30T18:00:00.123456+00:00",
            severity: "critical",
        },
    ],
};

const turnReport = (used: number): AgentEvent => ({
    type: "rate_limit",
    status: "allowed",
    resets_at: null,
    raw: { unifiedWindows: { five_hour: { status: "allowed", utilization: used } } },
});

const shares = (usage: AccountUsage | null) =>
    usage?.windows.map((window) => `${window.name} ${Math.round(window.used * 100)}`);

describe("the mock's account usage", () => {
    it("keeps a model's limit heard when asked through later reports after a turn", () => {
        const asked = heardUsage(sampleAccountUsage(), answer);
        const after = asked && heardUsage(asked, turnReport(0.5));
        expect(shares(after)).toEqual(["5-hour 50", "weekly 27", "weekly Fable 100"]);
        const fable = after?.windows.find((window) => window.kind === "seven_day_fable");
        expect(fable).toMatchObject({ asked: true, status: "critical", resets_at: 1_796_061_600 });
    });

    it("takes a report's unnamed model limit for the one model's limit known", () => {
        // Before Fable is named, the sample's model limit stands apart; asked, it is Fable's.
        expect(shares(sampleAccountUsage())).toContain("weekly model 40");
        const asked = heardUsage(sampleAccountUsage(), answer);
        const modelReport: AgentEvent = {
            type: "rate_limit",
            status: "allowed",
            resets_at: null,
            raw: {
                unifiedWindows: {
                    seven_day_overage_included: { status: "allowed", utilization: 0.4 },
                },
            },
        };
        const after = asked && heardUsage(asked, modelReport);
        expect(shares(after)).toEqual(["5-hour 36", "weekly 27", "weekly Fable 40"]);
        expect(after?.windows[2]).toMatchObject({ kind: "seven_day_fable", asked: false });
    });

    it("keeps the extra usage, and changes nothing for what is no report", () => {
        const extra: AgentEvent = {
            type: "extra_usage",
            enabled: true,
            used: 3871,
            limit: 40_000,
            currency: "USD",
            decimals: 2,
            limit_reached: false,
        };
        expect(heardUsage(sampleAccountUsage(), extra)?.extra?.used).toBe(3871);
        expect(heardUsage(sampleAccountUsage(), { type: "text", text: "hi" })).toBeNull();
    });

    it("keeps a report's refusal of extra usage through an answer, as the core does", () => {
        const report = (overageStatus: string): AgentEvent => ({
            type: "rate_limit",
            status: "allowed",
            resets_at: null,
            raw: { overageStatus },
        });
        const extra: AgentEvent = {
            type: "extra_usage",
            enabled: true,
            used: 3871,
            limit: 40_000,
            currency: "USD",
            decimals: 2,
            limit_reached: false,
        };
        const out = heardUsage(sampleAccountUsage(), report("rejected"));
        const asked = out && heardUsage(out, extra);
        expect(asked?.pays?.can).toBe(false);
        const added = asked && heardUsage(asked, report("allowed"));
        expect(added?.pays).toMatchObject({ can: true, asked: false });
    });
});

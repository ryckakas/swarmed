import { describe, expect, it } from "vitest";
import type { AgentEvent } from "./api/types";
import {
    type ExtraUsage,
    latestExtraUsage,
    money,
    onExtraUsageNow,
    spentShare,
} from "./extraUsage";

// What Claude Code answered for the account behind this change: euros, a cap of 400.
const extra = (used: number, limit: number | null = 40_000): ExtraUsage => ({
    type: "extra_usage",
    enabled: true,
    used,
    limit,
    currency: "EUR",
    decimals: 2,
    limit_reached: false,
});
const report = (raw: unknown): AgentEvent => ({
    type: "rate_limit",
    status: "rejected",
    resets_at: null,
    raw,
});

describe("extra usage", () => {
    it("takes the latest figures in the session", () => {
        expect(latestExtraUsage([extra(100), { type: "text", text: "hi" }, extra(3871)])).toEqual(
            extra(3871),
        );
        expect(latestExtraUsage([])).toBeNull();
    });

    it("says from the latest usage report whether the turn ran on it", () => {
        expect(onExtraUsageNow([report({ isUsingOverage: true })])).toBe(true);
        expect(onExtraUsageNow([report({ overageInUse: true })])).toBe(true);
        expect(onExtraUsageNow([report({ isUsingOverage: true }), report({})])).toBe(false);
        expect(onExtraUsageNow([])).toBe(false);
    });

    it("goes by the limits asked for after the report, until the next turn reports again", () => {
        const limits = (percent: number): AgentEvent => ({
            type: "usage_limits",
            limits: [
                { kind: "session", model: null, percent, resets_at: null, severity: "normal" },
                {
                    kind: "weekly_all",
                    model: null,
                    percent: 40,
                    resets_at: null,
                    severity: "normal",
                },
            ],
        });
        const billed = report({ isUsingOverage: true });
        // The window has reset since: the coins asked, and nothing is used up any more.
        expect(onExtraUsageNow([billed, limits(3)])).toBe(false);
        // Still used up when asked: the turns are still billed.
        expect(onExtraUsageNow([billed, limits(100)])).toBe(true);
        // Limits asked for before the report say nothing about it.
        expect(onExtraUsageNow([limits(3), billed])).toBe(true);
        // A turn that reports billed again, after the asking, is billed.
        expect(onExtraUsageNow([billed, limits(3), billed])).toBe(true);
    });

    it("writes minor units as money, with the cap's share", () => {
        expect(money(3871, "EUR", 2)).toMatch(/38[.,]71/);
        expect(money(3871, "EUR", 2)).toMatch(/€|EUR/);
        expect(money(5, "XYZ-NOT-A-CURRENCY", 2)).toBe("0.05 XYZ-NOT-A-CURRENCY");
        expect(spentShare(extra(3871))).toBe(10);
        expect(spentShare(extra(50_000))).toBe(100);
        expect(spentShare(extra(3871, null))).toBeNull();
    });
});

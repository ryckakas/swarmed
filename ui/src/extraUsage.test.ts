import { describe, expect, it } from "vitest";
import type { AccountUsage, UsageWindow } from "./api/types";
import { type ExtraUsage, money, onExtraUsageNow, spentShare } from "./extraUsage";

// What Claude Code answered for the account behind this change: euros, a cap of 400.
const extra = (used: number, limit: number | null = 40_000): ExtraUsage => ({
    enabled: true,
    used,
    limit,
    currency: "EUR",
    decimals: 2,
    limit_reached: false,
    heard_at: 0,
});

const now = 10_000;
/** A turn report heard at `at` that said whether the turn was billed, of a window resetting at `resets`. */
const billed = (on: boolean, at: number, resets: number | null = null): AccountUsage["billed"] => ({
    on,
    resets_at: resets,
    heard_at: at,
});
/** The 5-hour window as Claude Code gave it when asked at `at`. */
const asked = (used: number, at: number): UsageWindow => ({
    kind: "five_hour",
    name: "5-hour",
    used,
    resets_at: null,
    status: "allowed",
    heard_at: at,
    asked: true,
});
const usage = (turn: AccountUsage["billed"], ...windows: UsageWindow[]): AccountUsage => ({
    windows,
    extra: null,
    billed: turn,
});

describe("extra usage", () => {
    it("says from the latest report after a turn whether turns run on it", () => {
        expect(onExtraUsageNow(usage(billed(true, 100)), now)).toBe(true);
        expect(onExtraUsageNow(usage(billed(false, 100)), now)).toBe(false);
        expect(onExtraUsageNow(usage(null), now)).toBe(false);
        expect(onExtraUsageNow(null, now)).toBe(false);
    });

    it("goes by the limits asked for after the report, until the next turn reports again", () => {
        // The window has reset since: the coins asked, and nothing is used up any more.
        expect(onExtraUsageNow(usage(billed(true, 100), asked(0.03, 200)), now)).toBe(false);
        // Still used up when asked: the turns are still billed.
        expect(onExtraUsageNow(usage(billed(true, 100), asked(1, 200)), now)).toBe(true);
        // Limits asked for before the report say nothing about it.
        expect(onExtraUsageNow(usage(billed(true, 300), asked(0.03, 200)), now)).toBe(true);
    });

    it("ends when the limit it paid past resets, with nothing heard since", () => {
        expect(onExtraUsageNow(usage(billed(true, 100, now + 60)), now)).toBe(true);
        expect(onExtraUsageNow(usage(billed(true, 100, now - 60)), now)).toBe(false);
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

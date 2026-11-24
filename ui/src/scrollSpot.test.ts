import { describe, expect, it } from "vitest";
import {
    atBottom,
    FOLLOW_DISTANCE,
    FOLLOWING,
    rowAtTop,
    type ScrollSpot,
    scrollKey,
    scrollTarget,
    spotOf,
    withoutScrolls,
} from "./scrollSpot";

describe("a transcript's spot", () => {
    const view = 600;
    const height = 50_000;
    const bottom = height - view;

    it("follows at the bottom, and within the follow distance of it", () => {
        expect(atBottom({ top: bottom, height, view })).toBe(true);
        expect(atBottom({ top: bottom - FOLLOW_DISTANCE + 1, height, view })).toBe(true);
        expect(atBottom({ top: bottom - FOLLOW_DISTANCE, height, view })).toBe(false);
        expect(spotOf({ top: bottom, height, view }, { key: 7, top: bottom - 10 })).toEqual(
            FOLLOWING,
        );
        // A conversation shorter than the view is at its bottom.
        expect(atBottom({ top: 0, height: 300, view })).toBe(true);
    });

    it("is the row at the top of the view and how far down it, when scrolled up", () => {
        const spot = spotOf({ top: 12_340, height, view }, { key: 412, top: 12_300 });
        expect(spot).toEqual({ following: false, key: 412, offset: 40 });
        expect(spotOf({ top: 12_340, height, view }, null)).toEqual(FOLLOWING);
    });

    it("comes back to the same row wherever its measured top is now", () => {
        const spot: ScrollSpot = { following: false, key: 412, offset: 40 };
        // The rows above it had only an estimate, and are taller once measured.
        expect(scrollTarget(spot, 18_000)).toBe(18_040);
        expect(scrollTarget(FOLLOWING, 18_000)).toBe("bottom");
        // A row that is gone leaves the view where it is.
        expect(scrollTarget(spot, null)).toBeNull();
    });

    it("finds the row at the top of the view by halving", () => {
        // 2,000 rows, 30 pixels apart, from 16 down.
        const tops = Array.from({ length: 2000 }, (_, index) => 16 + index * 30);
        let asked = 0;
        const topOf = (index: number) => {
            asked += 1;
            return tops[index] ?? 0;
        };
        expect(rowAtTop(tops.length, topOf, 16 + 1234 * 30 + 5)).toBe(1234);
        expect(asked).toBeLessThanOrEqual(12);
        expect(rowAtTop(tops.length, topOf, 16 + 1234 * 30)).toBe(1234);
        expect(rowAtTop(tops.length, topOf, 0)).toBe(0);
        expect(rowAtTop(tops.length, topOf, 1e9)).toBe(1999);
        expect(rowAtTop(0, topOf, 100)).toBe(0);
    });
});

describe("the spots kept", () => {
    it("are kept per session, and per sub-agent under its session", () => {
        expect(scrollKey(12)).toBe("12");
        expect(scrollKey(12, "toolu_01")).toBe("12/toolu_01");
    });

    it("go with their sessions, sub-agents' included, and stay the same object otherwise", () => {
        const up: ScrollSpot = { following: false, key: 3, offset: 0 };
        const scrolls = { "12": up, "12/toolu_01": up, "120": up, "7": FOLLOWING };
        expect(withoutScrolls(scrolls, [12])).toEqual({ "120": up, "7": FOLLOWING });
        expect(withoutScrolls(scrolls, [99])).toBe(scrolls);
    });
});

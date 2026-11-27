import { describe, expect, it } from "vitest";
import {
    atBottom,
    FOLLOW_DISTANCE,
    FOLLOWING,
    type Look,
    placeFor,
    rowAtTop,
    type ScrollSpot,
    scrollKey,
    spotOf,
    withoutPlaces,
} from "./scrollSpot";

describe("a transcript's spot", () => {
    const view = 600;
    const height = 50_000;
    const bottom = height - view;

    it("follows at the bottom, and within the follow distance of it", () => {
        expect(atBottom({ top: bottom, height, view })).toBe(true);
        expect(atBottom({ top: bottom - FOLLOW_DISTANCE + 1, height, view })).toBe(true);
        expect(atBottom({ top: bottom - FOLLOW_DISTANCE, height, view })).toBe(false);
        expect(spotOf({ top: bottom, height, view }, { key: 7, top: bottom - 10 }, false)).toEqual(
            FOLLOWING,
        );
        // A conversation shorter than the view is at its bottom.
        expect(atBottom({ top: 0, height: 300, view })).toBe(true);
    });

    it("stops following at the first pixels of a scroll up, and not right at the bottom", () => {
        const row = { key: 1999, top: bottom - 30 };
        // Scrolled down to within the follow distance: following again.
        expect(spotOf({ top: bottom - 30, height, view }, row, false)).toEqual(FOLLOWING);
        // Scrolled up as far, while an answer is written: the user is leaving the bottom.
        expect(spotOf({ top: bottom - 30, height, view }, row, true)).toEqual({
            following: false,
            key: 1999,
            offset: 0,
        });
        // A view the browser moved up to the bottom, as rows below went, is at the bottom.
        expect(spotOf({ top: bottom, height, view }, row, true)).toEqual(FOLLOWING);
        expect(spotOf({ top: bottom - 0.5, height, view }, row, true)).toEqual(FOLLOWING);
    });

    it("is the row at the top of the view and how far down it, when scrolled up", () => {
        const spot = spotOf({ top: 12_340, height, view }, { key: 412, top: 12_300 }, true);
        expect(spot).toEqual({ following: false, key: 412, offset: 40 });
        expect(spotOf({ top: 12_340, height, view }, null, true)).toEqual(FOLLOWING);
    });
});

describe("where the view goes when rows change size", () => {
    const spot: ScrollSpot = { following: false, key: 412, offset: 40 };

    /** A look at a view scrolled to `top` and last known at `last`, with its rows' tops by key. */
    function look(top: number, last: number, rows: Record<number, number>, more?: Partial<Look>) {
        const tops = Object.entries(rows).map(([key, at]) => ({ key: Number(key), top: at }));
        return {
            first: false,
            scrolled: { top, height: 50_000, view: 600 },
            last,
            rowAtTop: () => tops.filter((row) => row.top <= top).pop() ?? null,
            rowTop: (key: number) => rows[key] ?? null,
            ...more,
        };
    }

    it("goes to the bottom when following", () => {
        expect(placeFor(FOLLOWING, look(900, 900, {}))).toEqual({ spot: FOLLOWING, top: "bottom" });
    });

    it("comes back to the same row on the first look, wherever its measured top is now", () => {
        // The rows above it had only an estimate, and are taller once measured.
        const first = { first: true };
        expect(placeFor(spot, look(0, 0, { 412: 18_000 }, first))).toEqual({ spot, top: 18_040 });
        // Already there, it stays.
        const there = look(18_040, 0, { 412: 18_000 }, first);
        expect(placeFor(spot, there)).toEqual({ spot, top: null });
        // A row that is gone shows the latest, and follows.
        expect(placeFor(spot, look(0, 0, {}, first))).toEqual({ spot: FOLLOWING, top: "bottom" });
    });

    it("goes the rest of the way once rows below are measured, if the browser held it short", () => {
        // Put at 18,040 on the first look, the view could only go as far as 17,900.
        expect(placeFor(spot, look(17_900, 17_900, { 412: 18_000 }))).toEqual({
            spot,
            top: 18_040,
        });
    });

    it("keeps to its row as rows above it are measured", () => {
        // They were measured 500 pixels taller than their estimate.
        const measured = look(12_340, 12_340, { 412: 12_800 });
        expect(placeFor(spot, measured)).toEqual({ spot, top: 12_840 });
        // Rows below it change nothing.
        expect(placeFor(spot, look(12_340, 12_340, { 412: 12_300 }))).toEqual({ spot, top: null });
        // A row that is gone after the first look leaves the view where it is.
        expect(placeFor(spot, look(12_340, 12_340, {}))).toEqual({ spot, top: null });
        // A browser that kept the view on the row itself has left nothing to do.
        expect(placeFor(spot, look(12_840, 12_340, { 412: 12_800 }))).toEqual({ spot, top: null });
    });

    it("never takes back a scroll the user made in a frame where a reply grew", () => {
        // The view stood 40 pixels down row 412, at 12,340. In one frame the user scrolled 40
        // pixels up and a piece of the reply below made the list taller. Going by the spot as
        // it was, the view would be put back at 12,340.
        const rows = { 411: 12_250, 412: 12_300 };
        const scrolled = look(12_300, 12_340, rows);
        const there: ScrollSpot = { following: false, key: 412, offset: 0 };
        expect(placeFor(spot, scrolled)).toEqual({ spot: there, top: null });
        // Its scroll event came first and was taken in: the same.
        expect(placeFor(there, look(12_300, 12_300, rows))).toEqual({ spot: there, top: null });
    });

    it("stops following when the user scrolls up while a reply is written", () => {
        // At the bottom, 49,400, the user scrolled up 10 pixels as the reply grew 24.
        const scrolled = { top: 49_390, height: 50_024, view: 600 };
        const growing = look(49_390, 49_400, { 1999: 49_380 }, { scrolled });
        expect(placeFor(FOLLOWING, growing)).toEqual({
            spot: { following: false, key: 1999, offset: 10 },
            top: null,
        });
    });
});

describe("the rows of a transcript", () => {
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
        expect(withoutPlaces(scrolls, [12])).toEqual({ "120": up, "7": FOLLOWING });
        expect(withoutPlaces(scrolls, [99])).toBe(scrolls);
    });
});

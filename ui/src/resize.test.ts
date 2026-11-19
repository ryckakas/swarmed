import { describe, expect, it } from "vitest";
import {
    clampWidth,
    KEY_STEP,
    MIN_MAIN_WIDTH,
    PANEL_WIDTH,
    SIDEBAR_WIDTH,
    widestPanel,
    widthAfterKey,
} from "./resize";

describe("clampWidth", () => {
    it("keeps a width within the limits and whole", () => {
        expect(clampWidth(300.4, SIDEBAR_WIDTH)).toBe(300);
        expect(clampWidth(10, SIDEBAR_WIDTH)).toBe(SIDEBAR_WIDTH.min);
        expect(clampWidth(5000, SIDEBAR_WIDTH)).toBe(SIDEBAR_WIDTH.max);
    });

    it("falls back to the initial width for what is not a number", () => {
        expect(clampWidth(Number.NaN, SIDEBAR_WIDTH)).toBe(SIDEBAR_WIDTH.initial);
    });
});

describe("widthAfterKey", () => {
    it("moves by a step with the arrows and to the limits with Home and End", () => {
        expect(widthAfterKey("ArrowRight", 300, SIDEBAR_WIDTH)).toBe(300 + KEY_STEP);
        expect(widthAfterKey("ArrowLeft", SIDEBAR_WIDTH.min, SIDEBAR_WIDTH)).toBe(
            SIDEBAR_WIDTH.min,
        );
        expect(widthAfterKey("Home", 300, SIDEBAR_WIDTH)).toBe(SIDEBAR_WIDTH.min);
        expect(widthAfterKey("End", 300, SIDEBAR_WIDTH)).toBe(SIDEBAR_WIDTH.max);
    });

    it("ignores other keys", () => {
        expect(widthAfterKey("a", 300, SIDEBAR_WIDTH)).toBeNull();
    });
});

describe("widestPanel", () => {
    it("leaves the conversation its minimum width", () => {
        expect(widestPanel(1600, 300)).toBe(1600 - 300 - MIN_MAIN_WIDTH);
    });

    it("never goes below the panel's own minimum in a small window", () => {
        expect(widestPanel(700, 300)).toBe(PANEL_WIDTH.min);
    });
});

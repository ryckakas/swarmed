import { describe, expect, it } from "vitest";
import { effortBars } from "./EffortPicker";

describe("effortBars", () => {
    it("fills one more bar for each step of effort", () => {
        const bars = ["low", "medium", "high", "xhigh", "max"].map(effortBars);
        expect(bars).toEqual([1, 2, 3, 4, 5]);
    });

    it("fills none for an effort that is not known", () => {
        expect(effortBars(null)).toBe(0);
        expect(effortBars("turbo")).toBe(0);
    });
});

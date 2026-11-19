import { describe, expect, it } from "vitest";
import { rootFontSize } from "./appearance";

describe("rootFontSize", () => {
    it("is the chosen scale in percent", () => {
        expect(rootFontSize(110)).toBe("110%");
    });

    it("falls back to the normal size for what cannot be shown", () => {
        expect(rootFontSize(undefined)).toBe("100%");
        expect(rootFontSize(900)).toBe("100%");
        expect(rootFontSize(Number.NaN)).toBe("100%");
    });
});

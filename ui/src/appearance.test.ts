import { describe, expect, it } from "vitest";
import type { Settings } from "./api/types";
import { applyAppearance, PALETTES, rootFontSize } from "./appearance";

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

describe("applyAppearance", () => {
    const page = () => ({ dataset: {}, style: {} }) as unknown as HTMLElement;
    const settings = { theme: "dark", palette: "plum", text_scale: 110 } as Settings;

    it("puts the theme, the palette and the text size on the page", () => {
        const root = page();
        applyAppearance(root, settings);
        expect(root.dataset).toEqual({ theme: "dark", palette: "plum" });
        expect(root.style.fontSize).toBe("110%");
        // The system's choice is the absence of a theme, and the palette still applies.
        applyAppearance(root, { ...settings, theme: "system", palette: "forest" });
        expect(root.dataset).toEqual({ palette: "forest" });
    });

    it("draws in the first palette until the settings are read", () => {
        const root = page();
        applyAppearance(root, null);
        expect(root.dataset).toEqual({ palette: "dusk" });
        expect(PALETTES[0]?.[0]).toBe("dusk");
    });
});

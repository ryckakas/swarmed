import { describe, expect, it } from "vitest";
import type { CustomLayout, Settings } from "./api/types";
import { customCss } from "./appearance";
import { drawnLayout, layoutCss } from "./customLayout";

describe("drawnLayout", () => {
    it("is the chosen layout, and for a custom one the layout it starts from", () => {
        expect(drawnLayout({ layout: "senior" } as Settings, null)).toBe("senior");
        expect(drawnLayout({ layout: "custom" } as Settings, { base: "senior" })).toBe("senior");
        // Chosen before it is read: classic until then.
        expect(drawnLayout({ layout: "custom" } as Settings, null)).toBe("classic");
        expect(drawnLayout(null, null)).toBe("classic");
    });
});

describe("layoutCss", () => {
    it("writes nothing for a layout that sets nothing", () => {
        expect(layoutCss({ base: "classic" })).toBe("");
    });

    it("sets sizes in rem, so that the text size setting still scales them", () => {
        const css = layoutCss({
            base: "classic",
            text: { sm: 16, xl: 24 },
            icons: 150,
            row_height: 40,
        });
        expect(css).toBe(
            ":root[data-custom] body { --text-sm: 1rem; --text-xl: 1.5rem; --icon-zoom: 1.5; --button-zoom: 1.5; --row-min: 40px; }\n",
        );
    });

    it("scales every step of spacing", () => {
        const css = layoutCss({ base: "classic", spacing: 150 });
        expect(css).toContain("--space-1: 6px; --space-2: 12px;");
        expect(css).toContain("--space-8: 60px;");
    });

    it("keeps the bundled fonts behind a font of the user's", () => {
        const css = layoutCss({ base: "classic", font: "Helvetica Neue", line_height: 160 });
        expect(css).toContain(
            '--font-ui: "Helvetica Neue", "Atkinson Hyperlegible Next Variable", system-ui',
        );
        expect(css).toContain("line-height: 1.6;");
    });

    it("draws dark colours only where the app is dark, and light ones where it is light", () => {
        const css = layoutCss({
            base: "classic",
            dark: { dusk: "#101010" },
            light: { dusk: "#fafafa" },
        });
        expect(css).toContain(':root[data-custom][data-theme="dark"] body { --dusk: #101010; }');
        expect(css).toContain(':root[data-custom][data-theme="light"] body { --dusk: #fafafa; }');
        expect(css).toContain(
            '@media (prefers-color-scheme: dark) { :root[data-custom]:not([data-theme="light"]) body { --dusk: #101010; }',
        );
        expect(css).toContain(
            '@media (prefers-color-scheme: light) { :root[data-custom]:not([data-theme="dark"]) body { --dusk: #fafafa; }',
        );
    });

    it("leaves out what is not of the form it knows, whatever let it through", () => {
        const hostile = {
            base: "classic",
            font: 'x"; } body { display: none',
            icons: "2; } * { display: none" as unknown as number,
            dark: { dusk: "red; } body { display: none", "x; y": "#000000", text: "#ffffff" },
        } as CustomLayout;
        expect(layoutCss(hostile)).toBe(
            ':root[data-custom][data-theme="dark"] body { --text: #ffffff; }\n' +
                '@media (prefers-color-scheme: dark) { :root[data-custom]:not([data-theme="light"]) body { --text: #ffffff; }\n }\n',
        );
    });
});

describe("customCss", () => {
    it("is empty unless the settings choose the custom layout", () => {
        const custom: CustomLayout = { base: "classic", icons: 120 };
        expect(customCss({ layout: "senior" } as Settings, custom)).toBe("");
        expect(customCss({ layout: "custom" } as Settings, null)).toBe("");
        expect(customCss({ layout: "custom" } as Settings, custom)).toContain("--icon-zoom: 1.2");
    });
});

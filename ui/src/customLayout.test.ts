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
        expect(layoutCss({ base: "senior", sidebar: { task: {}, session: { dark: {} } } })).toBe(
            "",
        );
    });

    it("sets each level of the sidebar apart by its own tokens", () => {
        const css = layoutCss({
            base: "classic",
            sidebar: {
                repository: { size: 18, weight: 600 },
                sub_agent: { size: 12, dark: { colour: "#c0c0c0", background: "#202020" } },
                session: { light: { colour: "#101010" } },
            },
        });
        expect(css).toContain(
            ":root[data-custom] body { --sidebar-repository-size: 1.125rem; --sidebar-repository-weight: 600; --sidebar-sub-agent-size: 0.75rem; }",
        );
        expect(css).toContain(
            ':root[data-custom][data-theme="dark"] body { --sidebar-sub-agent-colour: #c0c0c0; --sidebar-sub-agent-background: #202020; }',
        );
        expect(css).toContain(
            ':root[data-custom][data-theme="light"] body { --sidebar-session-colour: #101010; }',
        );
        expect(css).toContain(
            '@media (prefers-color-scheme: light) { :root[data-custom]:not([data-theme="dark"]) body { --sidebar-session-colour: #101010; }',
        );
    });

    it("keeps a session's row within its own colours whatever it is doing", () => {
        const css = layoutCss({
            base: "classic",
            sidebar: { session: { dark: { colour: "#111111", background: "#f0f0f0" } } },
        });
        // Pointed at, open or waiting, it stays on its own background, tinted with its colour:
        // the palette's were never checked against it. An unread title keeps the colour too.
        expect(css).toContain("--sidebar-session-pointed: color-mix(in srgb, #111111 6%, #f0f0f0)");
        expect(css).toContain("--sidebar-session-open: color-mix(in srgb, #111111 12%, #f0f0f0)");
        expect(css).toContain("--sidebar-session-unread: #111111");
        // A colour alone is drawn on the sidebar's own backgrounds, as before.
        const alone = layoutCss({
            base: "classic",
            sidebar: {
                session: { dark: { colour: "#e0e0e0" } },
                task: { dark: { colour: "#e0e0e0", background: "#101010" } },
            },
        });
        expect(alone).not.toContain("-pointed");
        expect(alone).not.toContain("-unread");
    });

    it("writes a level's colours beside the form's own", () => {
        const css = layoutCss({
            base: "classic",
            dark: { panel: "#181818" },
            sidebar: { task: { dark: { colour: "#e0e0e0" } } },
        });
        expect(css).toContain(
            ':root[data-custom][data-theme="dark"] body { --panel: #181818; --sidebar-task-colour: #e0e0e0; }',
        );
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

    it("gives marks and highlights the colours of their names", () => {
        const css = layoutCss({ base: "classic", dark: { green: "#33ff99", purple: "#bb88ff" } });
        expect(css).toContain(
            ':root[data-custom][data-theme="dark"] body { --green: #33ff99; --purple: #bb88ff; }',
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

    it("leaves out a level's values that are not of the form it knows", () => {
        const hostile = {
            base: "classic",
            sidebar: {
                session: {
                    size: "2; } * { display: none",
                    weight: 600.5,
                    dark: { colour: "red; } body { display: none", background: 12 },
                },
                task: { weight: "700; } * { display: none", light: "#ffffff" },
                "x; } * { display: none": { size: 14 },
            },
        } as unknown as CustomLayout;
        expect(layoutCss(hostile)).toBe("");
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

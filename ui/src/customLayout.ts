// The user's custom layout, turned into the stylesheet that draws it. Pure, so it can be tested.
//
// The core has checked every value before it gets here: numbers are within their ranges,
// colours are `#rrggbb`, a font's name holds nothing but letters, digits, spaces and hyphens.
// This file still writes only what it knows the form of, so that nothing else can reach the
// page by way of a layout.

import type { CustomLayout, Layout, LayoutColours, Settings, SidebarLevelName } from "./api/types";

/** The layout whose stylesheet rules apply: for a custom layout, the one it starts from. */
export function drawnLayout(
    settings: Settings | null,
    custom: CustomLayout | null,
): Exclude<Layout, "custom"> {
    const layout = settings?.layout ?? "classic";
    if (layout !== "custom") {
        return layout;
    }
    return custom?.base ?? "classic";
}

/** The classic layout's steps of spacing, in pixels, which a custom layout scales. */
const SPACE_STEPS: [string, number][] = [
    ["1", 4],
    ["2", 8],
    ["3", 12],
    ["4", 16],
    ["6", 24],
    ["8", 40],
];

const UI_FONTS = '"Atkinson Hyperlegible Next Variable", system-ui, sans-serif';
const MONO_FONTS = '"Atkinson Hyperlegible Mono Variable", ui-monospace, monospace';

const HEX = /^#[0-9a-f]{6}$/i;
const FONT_NAME = /^[\p{L}\p{N} ._-]{1,60}$/u;

/** The sidebar's levels by the core's names, each with the name its tokens have in app.css. */
const LEVELS: [SidebarLevelName, string][] = [
    ["repository", "repository"],
    ["task", "task"],
    ["session", "session"],
    ["sub_agent", "sub-agent"],
];

/** A size in pixels as the rem it is at the normal text size, so the text size setting scales it. */
function rem(pixels: number): string {
    return `${pixels / 16}rem`;
}

function numbered(value: number | undefined): value is number {
    return typeof value === "number" && Number.isFinite(value);
}

/** The declarations for the sizes, the spacing and the fonts. */
function measures(custom: CustomLayout): string[] {
    const lines: string[] = [];
    for (const step of ["xs", "sm", "md", "lg", "xl"] as const) {
        const pixels = custom.text?.[step];
        if (numbered(pixels)) {
            lines.push(`--text-${step}: ${rem(pixels)}`);
        }
    }
    if (numbered(custom.spacing)) {
        const factor = custom.spacing / 100;
        lines.push(
            ...SPACE_STEPS.map(
                ([name, pixels]) => `--space-${name}: ${Math.round(pixels * factor)}px`,
            ),
        );
    }
    if (numbered(custom.icons)) {
        lines.push(`--icon-zoom: ${custom.icons / 100}`, `--button-zoom: ${custom.icons / 100}`);
    }
    lines.push(...heights(custom), ...fonts(custom), ...levelMeasures(custom));
    return lines;
}

function heights(custom: CustomLayout): string[] {
    const lines: string[] = [];
    if (numbered(custom.row_height)) {
        lines.push(`--row-min: ${custom.row_height}px`);
    }
    if (numbered(custom.control_height)) {
        lines.push(`--control-min: ${custom.control_height}px`);
    }
    if (numbered(custom.line_height)) {
        lines.push(`line-height: ${custom.line_height / 100}`);
    }
    return lines;
}

function fonts(custom: CustomLayout): string[] {
    const lines: string[] = [];
    if (custom.font && FONT_NAME.test(custom.font)) {
        lines.push(`--font-ui: "${custom.font}", ${UI_FONTS}`);
    }
    if (custom.mono_font && FONT_NAME.test(custom.mono_font)) {
        lines.push(`--font-mono: "${custom.mono_font}", ${MONO_FONTS}`);
    }
    return lines;
}

/** The declarations for the size and weight of each level of the sidebar that sets them. */
function levelMeasures(custom: CustomLayout): string[] {
    const lines: string[] = [];
    for (const [name, token] of LEVELS) {
        const level = custom.sidebar?.[name];
        const size = level?.size;
        const weight = level?.weight;
        if (numbered(size)) {
            lines.push(`--sidebar-${token}-size: ${rem(size)}`);
        }
        if (numbered(weight) && Number.isInteger(weight)) {
            lines.push(`--sidebar-${token}-weight: ${weight}`);
        }
    }
    return lines;
}

/** The declarations for the colours of one form of the app. */
function colours(given: LayoutColours | undefined): string[] {
    return Object.entries(given ?? {})
        .filter(([name, value]) => /^[a-z]+$/.test(name) && HEX.test(value))
        .map(([name, value]) => `--${name}: ${value}`);
}

function hex(value: unknown): value is string {
    return typeof value === "string" && HEX.test(value);
}

/** The declarations for the colours of the sidebar's levels in one form of the app. */
function levelColours(custom: CustomLayout, form: "dark" | "light"): string[] {
    const lines: string[] = [];
    for (const [name, token] of LEVELS) {
        const given = custom.sidebar?.[name]?.[form];
        for (const part of ["colour", "background"] as const) {
            const value = given?.[part];
            if (hex(value)) {
                lines.push(`--sidebar-${token}-${part}: ${value}`);
            }
        }
    }
    return lines;
}

function rule(selector: string, lines: string[]): string {
    return lines.length === 0 ? "" : `${selector} { ${lines.join("; ")}; }\n`;
}

/**
 * The stylesheet of a custom layout. Its values sit on the body, under the root where the
 * palettes set theirs, and so take their place whichever palette is chosen. Dark colours apply
 * where the app is dark and light ones where it is light, as the palettes' own do.
 */
export function layoutCss(custom: CustomLayout): string {
    const body = ":root[data-custom] body";
    const dark = [...colours(custom.dark), ...levelColours(custom, "dark")];
    const light = [...colours(custom.light), ...levelColours(custom, "light")];
    return [
        rule(body, measures(custom)),
        rule(':root[data-custom][data-theme="dark"] body', dark),
        rule(':root[data-custom][data-theme="light"] body', light),
        media("dark", rule(':root[data-custom]:not([data-theme="light"]) body', dark)),
        media("light", rule(':root[data-custom]:not([data-theme="dark"]) body', light)),
    ].join("");
}

function media(scheme: "dark" | "light", inner: string): string {
    return inner === "" ? "" : `@media (prefers-color-scheme: ${scheme}) { ${inner} }\n`;
}

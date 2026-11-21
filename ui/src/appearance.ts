import type { CustomLayout, Layout, Palette, Settings, Style } from "./api/types";
import { drawnLayout, layoutCss } from "./customLayout";

// How the user's appearance settings reach the page. The stylesheet does the rest.

/** The root font size, in percent, for a text scale; anything unusable is the normal size. */
export function rootFontSize(scale: number | undefined): string {
    const usable = typeof scale === "number" && scale >= 80 && scale <= 150;
    return `${usable ? scale : 100}%`;
}

/** The palettes on offer, by what the stylesheet knows them as, with the names to show. */
export const PALETTES: [Palette, string][] = [
    ["swarm", "Swarm"],
    ["dusk", "Dusk"],
    ["graphite", "Graphite"],
    ["forest", "Forest"],
    ["plum", "Plum"],
    ["ember", "Ember"],
];

/** The palette the app is drawn in until the user chooses another. */
export const DEFAULT_PALETTE: Palette = "swarm";

/** The icons and words on offer, with the names to show. */
export const STYLES: [Style, string][] = [
    ["colony", "True swarm"],
    ["swarm", "Weak swarm"],
    ["plain", "Boring"],
];

/** The icons and words the app uses until the user chooses others. */
export const DEFAULT_STYLE: Style = "colony";

/** Whether a style draws the swarm's icons; before the settings are read, the default does. */
export function isSwarm(style: Style | undefined): boolean {
    return (style ?? DEFAULT_STYLE) !== "plain";
}

/** The layouts on offer, with the names to show and what each is for. */
export const LAYOUTS: [Layout, string, string][] = [
    ["classic", "Classic", "Dense, with small text: the most sessions in view."],
    ["senior", "Senior", "Larger text, taller rows, larger buttons, stronger contrast."],
    ["custom", "Custom", "Your own sizes, spacing, fonts and colours, as JSON."],
];

/** The layout the app is drawn in until the user chooses another. */
export const DEFAULT_LAYOUT: Layout = "classic";

/** The stylesheet a custom layout is drawn with: empty unless the settings choose that layout. */
export function customCss(settings: Settings | null, custom: CustomLayout | null): string {
    return settings?.layout === "custom" && custom ? layoutCss(custom) : "";
}

/**
 * Puts the theme, the palette, the style, the layout and the text size on the page. Before the
 * settings are read, the defaults. A custom layout is drawn as the layout it starts from, with
 * its own stylesheet on top, which `drawCustom` puts in place.
 */
export function applyAppearance(
    root: HTMLElement,
    settings: Settings | null,
    custom: CustomLayout | null = null,
) {
    const theme = settings?.theme ?? "system";
    if (theme === "system") {
        delete root.dataset.theme;
    } else {
        root.dataset.theme = theme;
    }
    root.dataset.palette = settings?.palette ?? DEFAULT_PALETTE;
    // The stylesheet knows two: the renamed tree only changes words.
    root.dataset.style = isSwarm(settings?.style) ? "swarm" : "plain";
    root.dataset.layout = drawnLayout(settings, custom);
    if (settings?.layout === "custom") {
        root.dataset.custom = "";
    } else {
        delete root.dataset.custom;
    }
    root.style.fontSize = rootFontSize(settings?.text_scale);
}

/** Keeps one style element in `page` holding `css`, the custom layout's stylesheet. */
export function drawCustom(page: Document, css: string) {
    const id = "custom-layout";
    const held =
        page.getElementById(id) ??
        page.head.appendChild(Object.assign(page.createElement("style"), { id }));
    if (held.textContent !== css) {
        held.textContent = css;
    }
}

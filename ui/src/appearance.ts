import type { Palette, Settings, Style } from "./api/types";

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
    ["swarm", "Swarm"],
    ["colony", "Swarm, tree renamed too"],
    ["plain", "Plain"],
];

/** The icons and words the app uses until the user chooses others. */
export const DEFAULT_STYLE: Style = "swarm";

/** Whether a style draws the swarm's icons; before the settings are read, the default does. */
export function isSwarm(style: Style | undefined): boolean {
    return (style ?? DEFAULT_STYLE) !== "plain";
}

/**
 * Puts the theme, the palette, the style and the text size on the page. Before the settings are read, the
 * defaults.
 */
export function applyAppearance(root: HTMLElement, settings: Settings | null) {
    const theme = settings?.theme ?? "system";
    if (theme === "system") {
        delete root.dataset.theme;
    } else {
        root.dataset.theme = theme;
    }
    root.dataset.palette = settings?.palette ?? DEFAULT_PALETTE;
    // The stylesheet knows two: the renamed tree only changes words.
    root.dataset.style = isSwarm(settings?.style) ? "swarm" : "plain";
    root.style.fontSize = rootFontSize(settings?.text_scale);
}

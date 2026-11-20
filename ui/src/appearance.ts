import type { Palette, Settings } from "./api/types";

// How the user's appearance settings reach the page. The stylesheet does the rest.

/** The root font size, in percent, for a text scale; anything unusable is the normal size. */
export function rootFontSize(scale: number | undefined): string {
    const usable = typeof scale === "number" && scale >= 80 && scale <= 150;
    return `${usable ? scale : 100}%`;
}

/** The palettes on offer, by what the stylesheet knows them as, with the names to show. */
export const PALETTES: [Palette, string][] = [
    ["dusk", "Dusk"],
    ["graphite", "Graphite"],
    ["forest", "Forest"],
    ["plum", "Plum"],
    ["ember", "Ember"],
    ["swarm", "Swarm"],
];

/**
 * Puts the theme, the palette and the text size on the page. Before the settings are read, the
 * defaults.
 */
export function applyAppearance(root: HTMLElement, settings: Settings | null) {
    const theme = settings?.theme ?? "system";
    if (theme === "system") {
        delete root.dataset.theme;
    } else {
        root.dataset.theme = theme;
    }
    root.dataset.palette = settings?.palette ?? "dusk";
    root.style.fontSize = rootFontSize(settings?.text_scale);
}

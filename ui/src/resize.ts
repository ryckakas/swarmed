// Widths of the panels a user can drag wider or narrower. Pure, so it can be tested.

export interface WidthLimits {
    /** The width a panel has until the user changes it, and after a double click. */
    initial: number;
    min: number;
    max: number;
}

export const SIDEBAR_WIDTH: WidthLimits = { initial: 272, min: 200, max: 560 };

/**
 * The task panel may be dragged very wide, for reading diffs; the stylesheet stops it where the
 * conversation would get too narrow for the window at hand.
 */
export const PANEL_WIDTH: WidthLimits = { initial: 380, min: 280, max: 2400 };

/** How far one arrow-key press moves a panel's edge, in pixels. */
export const KEY_STEP = 16;

/** `width` kept within the limits; anything that is not a number becomes the initial width. */
export function clampWidth(width: number, limits: WidthLimits): number {
    if (!Number.isFinite(width)) {
        return limits.initial;
    }
    return Math.round(Math.min(limits.max, Math.max(limits.min, width)));
}

/** The width a key press on a panel's edge asks for, or `null` if the key means nothing here. */
export function widthAfterKey(key: string, width: number, limits: WidthLimits): number | null {
    const moves: Record<string, number> = {
        ArrowLeft: width - KEY_STEP,
        ArrowRight: width + KEY_STEP,
        Home: limits.min,
        End: limits.max,
    };
    const wanted = moves[key];
    return wanted === undefined ? null : clampWidth(wanted, limits);
}

/** The narrowest the conversation may get when a panel beside it is dragged wider. */
export const MIN_MAIN_WIDTH = 360;

/** The widest a panel can be shown in a window, given what the other columns need. */
export function widestPanel(windowWidth: number, sidebarWidth: number): number {
    return Math.max(PANEL_WIDTH.min, windowWidth - sidebarWidth - MIN_MAIN_WIDTH);
}

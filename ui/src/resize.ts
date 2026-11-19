// Widths of the panels a user can drag wider or narrower. Pure, so it can be tested.

export interface WidthLimits {
    /** The width a panel has until the user changes it, and after a double click. */
    initial: number;
    min: number;
    max: number;
}

export const SIDEBAR_WIDTH: WidthLimits = { initial: 272, min: 200, max: 560 };

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

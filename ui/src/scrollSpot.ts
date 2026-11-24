// Where a transcript was left, so that a session opened again comes back to it. A position in
// pixels would not do: rows out of view have only an estimated height (`content-visibility`),
// so the same `scrollTop` lands somewhere else once a transcript is drawn again. What is kept
// instead is the row at the top of the view and how far down that row the view was, and the
// view goes back to that row once it has been measured. Kept per session, and per sub-agent,
// in the store and never on disk: after a restart a session opens at the bottom.

/** How close to the bottom, in pixels, still counts as following the conversation. */
export const FOLLOW_DISTANCE = 80;

/** Where a transcript stands: at the bottom, following what arrives, or at one of its rows. */
export type ScrollSpot =
    | { following: true }
    | {
          following: false;
          /** The row at the top of the view, by its item's key. */
          key: number;
          /** How far below that row's top the view's top was, in pixels. */
          offset: number;
      };

/** At the bottom, following what arrives: where a session never opened starts. */
export const FOLLOWING: ScrollSpot = { following: true };

/** How a scroller stands, in pixels. */
export interface Scrolled {
    /** How far it is scrolled down (`scrollTop`). */
    top: number;
    /** How tall what it holds is (`scrollHeight`). */
    height: number;
    /** How tall the part in view is (`clientHeight`). */
    view: number;
}

/** Whether the view is at the bottom, or close enough to it to follow what arrives. */
export function atBottom({ top, height, view }: Scrolled): boolean {
    return height - top - view < FOLLOW_DISTANCE;
}

/** A row as it is laid out: its item's key, and its top in the scroller's content. */
export interface Row {
    key: number;
    top: number;
}

/**
 * Where a transcript stands, from how it is scrolled and the row at the top of its view
 * (`null` if there is none to name). At the bottom it follows, wherever its rows are.
 */
export function spotOf(scrolled: Scrolled, row: Row | null): ScrollSpot {
    if (atBottom(scrolled) || row === null) {
        return FOLLOWING;
    }
    return { following: false, key: row.key, offset: scrolled.top - row.top };
}

/**
 * Where to scroll to stand at `spot` again, given where its row's top is now (`null` if the
 * row is not there): `"bottom"` to follow, the row's top and the offset for a row, and `null`
 * when the row is gone, so the view stays where it is.
 */
export function scrollTarget(spot: ScrollSpot, rowTop: number | null): number | "bottom" | null {
    if (spot.following) {
        return "bottom";
    }
    return rowTop === null ? null : rowTop + spot.offset;
}

/**
 * Which of `count` rows, in order from the top, is at the top of a view scrolled to `top`:
 * the last whose top is at or above it, found by halving, since a transcript can have
 * thousands of rows and is measured on every scroll. 0 when the first is below it too.
 */
export function rowAtTop(count: number, topOf: (index: number) => number, top: number): number {
    let low = 0;
    let high = count - 1;
    let found = 0;
    while (low <= high) {
        const middle = Math.floor((low + high) / 2);
        if (topOf(middle) <= top) {
            found = middle;
            low = middle + 1;
        } else {
            high = middle - 1;
        }
    }
    return found;
}

/** What the store keeps a transcript's spot under: its session, and its sub-agent if it is one's. */
export function scrollKey(sessionId: number, subAgent: string | null = null): string {
    return subAgent === null ? String(sessionId) : `${sessionId}/${subAgent}`;
}

/** `scrolls` without the spots of sessions `sessionIds`, sub-agents' included; the same object if it had none. */
export function withoutScrolls(
    scrolls: Record<string, ScrollSpot>,
    sessionIds: number[],
): Record<string, ScrollSpot> {
    const gone = new Set(sessionIds.map(String));
    const keys = Object.keys(scrolls);
    const kept = keys.filter((key) => !gone.has(key.split("/")[0] ?? key));
    if (kept.length === keys.length) {
        return scrolls;
    }
    return Object.fromEntries(kept.map((key) => [key, scrolls[key] ?? FOLLOWING]));
}

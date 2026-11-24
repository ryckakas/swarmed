// Where a transcript was left, so that a session opened again comes back to it. A position in
// pixels would not do: rows out of view have only an estimated height (`content-visibility`),
// so the same `scrollTop` lands somewhere else once a transcript is drawn again. What is kept
// instead is the row at the top of the view and how far down that row the view was, and the
// view goes back to that row once it has been measured. It goes back only after every move
// of the user's is taken in, so that none is ever taken back. Kept per session, and per
// sub-agent, in the store and never on disk: after a restart a session opens at the bottom.

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

/**
 * Whether the view is at the bottom, or less than `within` pixels from it: by default close
 * enough to follow what arrives.
 */
export function atBottom({ top, height, view }: Scrolled, within = FOLLOW_DISTANCE): boolean {
    return height - top - view < within;
}

/** A row as it is laid out: its item's key, and its top in the scroller's content. */
export interface Row {
    key: number;
    top: number;
}

/**
 * Where a transcript stands once its view moved by a scroll, from how it is scrolled, the row at
 * the top of its view (`null` if there is none to name) and whether the scroll went `up`. At the
 * bottom it follows, wherever its rows are. Within the follow distance of it too, unless the
 * scroll went up and the view is not right at the bottom: while an answer is written, the
 * first pixels of a scroll up are the user leaving the bottom, and following would pull the
 * view back down with every piece that arrives.
 */
export function spotOf(scrolled: Scrolled, row: Row | null, up: boolean): ScrollSpot {
    // Less than a pixel is the bottom: a scroller can stop a fraction short of it.
    const following = up ? atBottom(scrolled, 1) : atBottom(scrolled);
    if (following || row === null) {
        return FOLLOWING;
    }
    return { following: false, key: row.key, offset: scrolled.top - row.top };
}

/** What a look at a transcript whose rows changed size finds, in pixels down its content. */
export interface Look {
    /** Whether it is the first look since the transcript was drawn. */
    first: boolean;
    /** How it is scrolled now. */
    scrolled: Scrolled;
    /** Where the view was last known to be: put there for its spot, or scrolled there. */
    last: number;
    /** The row at the top of the view now; asked only when the view moved. */
    rowAtTop(): Row | null;
    /** Where the row of an item is now, by its key, or `null` when it is not drawn. */
    rowTop(key: number): number | null;
}

/** Where a look puts the view: at the bottom, at a point, or nowhere, leaving it where it is. */
export type Placement = number | "bottom" | null;

/**
 * Where the view goes for `spot` now that rows changed size, and the spot it then stands at.
 * A view that is not where it was last known to be was moved by the user, or by the browser
 * keeping what is in view in place as rows above it changed size (`overflow-anchor`), and
 * stands at a spot of its own even if its scroll event is still to come: that scroll is taken
 * in first, or the view would be put back, as when it came in the same frame as a reply's
 * piece. Then the view goes to the bottom to follow, or to its row's top and the offset, once
 * that row is measured. A row that is not drawn leaves the view where it is, except on the
 * first look, which then shows the latest.
 */
export function placeFor(spot: ScrollSpot, look: Look): { spot: ScrollSpot; top: Placement } {
    const { first, scrolled, last } = look;
    const moved = !first && scrolled.top !== last;
    const at = moved ? spotOf(scrolled, look.rowAtTop(), scrolled.top < last) : spot;
    if (at.following) {
        return { spot: at, top: "bottom" };
    }
    const rowTop = look.rowTop(at.key);
    if (rowTop === null) {
        return first ? { spot: FOLLOWING, top: "bottom" } : { spot: at, top: null };
    }
    const target = rowTop + at.offset;
    return { spot: at, top: Math.abs(target - scrolled.top) >= 1 ? target : null };
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

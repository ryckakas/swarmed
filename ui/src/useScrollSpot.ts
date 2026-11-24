import { type RefObject, useLayoutEffect, useRef, useState } from "react";
import {
    FOLLOWING,
    type Row,
    rowAtTop,
    type Scrolled,
    type ScrollSpot,
    scrollTarget,
    spotOf,
} from "./scrollSpot";
import { useStore } from "./store";

/** What a transcript's scrolling comes to: whether it is at the bottom, and the way back there. */
export interface ScrollControl {
    /** Whether the view is at the bottom, following what arrives. */
    atEnd: boolean;
    /** Goes to the latest message and follows the conversation again. */
    toLatest(): void;
    /** For the scroller's `onScroll`. */
    onScroll(): void;
}

/**
 * Keeps a transcript where its reader left it: at the bottom, following what arrives, or at the
 * row they scrolled to, kept under `place` when the transcript goes and taken up again when it
 * comes back. Rows out of view have only an estimated height, so the list grows as they are
 * measured; watching its size keeps the view at its spot through that too.
 */
export function useScrollSpot(
    scroller: RefObject<HTMLDivElement | null>,
    sessionId: number,
    place: string,
): ScrollControl {
    // Read once: the caller keys each transcript by its session or sub-agent, so it is new for each.
    const spot = useRef<ScrollSpot>(useStore.getState().scrolls[place] ?? FOLLOWING);
    const [atEnd, setAtEnd] = useState(spot.current.following);
    const knownHeight = useRef(0);
    // Where this hook last put the view: the scroll event that follows is its own, not the user's.
    const placed = useRef<number | null>(null);

    useLayoutEffect(() => {
        const element = scroller.current;
        const list = element?.firstElementChild;
        if (!(element && list)) {
            return;
        }
        let first = true;
        const settle = () => {
            const placing = placeFor(element, list, spot.current, first);
            first = false;
            if (placing.spot !== spot.current) {
                spot.current = placing.spot;
                setAtEnd(placing.spot.following);
            }
            if (placing.top !== null) {
                placed.current = put(element, placing.top);
            }
            knownHeight.current = element.scrollHeight;
        };
        settle();
        const observer = new ResizeObserver(settle);
        observer.observe(list);
        return () => {
            observer.disconnect();
            useStore.getState().keepScroll(sessionId, place, spot.current);
        };
    }, [scroller, sessionId, place]);

    const onScroll = () => {
        const element = scroller.current;
        const list = element?.firstElementChild;
        // A scroll event with a new content height is the browser adjusting to rows being
        // measured, and one to where the view was put is this hook's own: only the user moves
        // the spot.
        const own = element?.scrollTop === placed.current;
        if (!(element && list) || element.scrollHeight !== knownHeight.current || own) {
            return;
        }
        placed.current = null;
        spot.current = spotOf(scrolledOf(element), rowAt(element, list));
        setAtEnd(spot.current.following);
    };

    const toLatest = () => {
        spot.current = FOLLOWING;
        setAtEnd(true);
        if (scroller.current) {
            placed.current = put(scroller.current, scroller.current.scrollHeight);
        }
    };

    return { atEnd, toLatest, onScroll };
}

/** Scrolls to `top`, and says where that came to, which the browser keeps within bounds. */
function put(element: HTMLElement, top: number): number {
    element.scrollTop = top;
    return element.scrollTop;
}

/**
 * Where the view goes for `spot` now that rows changed size, and the spot it then stands at:
 * the bottom to follow; the spot's row, only if that moved, which leaves a scroll the user has
 * under way alone; nowhere if the row is gone. On the first look, a row that is gone shows
 * the latest instead.
 */
function placeFor(
    element: HTMLElement,
    list: Element,
    spot: ScrollSpot,
    first: boolean,
): { spot: ScrollSpot; top: number | null } {
    const target = scrollTarget(spot, topOfRow(element, list, spot));
    if (target === "bottom" || (target === null && first)) {
        return { spot: spot.following ? spot : FOLLOWING, top: element.scrollHeight };
    }
    const moved = target !== null && Math.abs(element.scrollTop - target) >= 1;
    return { spot, top: moved ? target : null };
}

function scrolledOf(element: HTMLElement): Scrolled {
    return { top: element.scrollTop, height: element.scrollHeight, view: element.clientHeight };
}

/** Where an element's top is in the scroller's content, whichever element it is positioned by. */
function contentTop(element: HTMLElement, row: Element): number {
    return (
        row.getBoundingClientRect().top - element.getBoundingClientRect().top + element.scrollTop
    );
}

/** The row at the top of the view, if it is one of the conversation's, which name their items. */
function rowAt(element: HTMLElement, list: Element): Row | null {
    const rows = list.children;
    const topOf = (index: number) => {
        const row = rows[index];
        return row ? contentTop(element, row) : 0;
    };
    const index = rowAtTop(rows.length, topOf, element.scrollTop);
    const key = rows[index]?.getAttribute("data-key");
    return key ? { key: Number(key), top: topOf(index) } : null;
}

/** Where the row a spot names is now, or `null` when it follows or the row is not drawn. */
function topOfRow(element: HTMLElement, list: Element, spot: ScrollSpot): number | null {
    if (spot.following) {
        return null;
    }
    const row = list.querySelector(`:scope > [data-key="${spot.key}"]`);
    return row ? contentTop(element, row) : null;
}

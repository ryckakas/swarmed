import { type RefObject, useLayoutEffect, useRef, useState } from "react";
import {
    FOLLOWING,
    placeFor,
    type Row,
    rowAtTop,
    type Scrolled,
    type ScrollSpot,
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
 * measured; watching its size keeps the view at its spot through that too. Every move of the
 * view the hook did not make stands for a spot of its own: taken in when its scroll event
 * comes, or before the view is placed if rows changed size first (`placeFor`).
 */
export function useScrollSpot(
    scroller: RefObject<HTMLDivElement | null>,
    sessionId: number,
    place: string,
): ScrollControl {
    // Read once: the caller keys each transcript by its session or sub-agent, so it is new for each.
    const kept = useRef<Kept>({ spot: useStore.getState().scrolls[place] ?? FOLLOWING, last: 0 });
    const [atEnd, setAtEnd] = useState(kept.current.spot.following);

    useLayoutEffect(() => {
        const element = scroller.current;
        const list = element?.firstElementChild;
        if (!(element && list)) {
            return;
        }
        let first = true;
        const settle = () => {
            placeView(element, list, kept.current, first);
            first = false;
            setAtEnd(kept.current.spot.following);
        };
        settle();
        const observer = new ResizeObserver(settle);
        observer.observe(list);
        return () => {
            observer.disconnect();
            useStore.getState().keepScroll(sessionId, place, kept.current.spot);
        };
    }, [scroller, sessionId, place]);

    const onScroll = () => {
        const element = scroller.current;
        const list = element?.firstElementChild;
        // One to where the view was last known to be says nothing new: the hook put it there,
        // or took that move in already. Any other is taken in, even in a frame where rows grew.
        if (!(element && list) || element.scrollTop === kept.current.last) {
            return;
        }
        const top = element.scrollTop;
        const up = top < kept.current.last;
        kept.current = { spot: spotOf(scrolledOf(element), rowAt(element, list), up), last: top };
        setAtEnd(kept.current.spot.following);
    };

    const toLatest = () => {
        setAtEnd(true);
        const element = scroller.current;
        const last = element ? put(element, element.scrollHeight) : kept.current.last;
        kept.current = { spot: FOLLOWING, last };
    };

    return { atEnd, toLatest, onScroll };
}

/** What the hook keeps between looks. */
interface Kept {
    /** Where the transcript stands. */
    spot: ScrollSpot;
    /** Where the view was last known to be: put there by the hook, or scrolled there. */
    last: number;
}

/** Puts the view where `placeFor` says now that rows changed size, and keeps what that came to. */
function placeView(element: HTMLElement, list: Element, kept: Kept, first: boolean) {
    const placing = placeFor(kept.spot, {
        first,
        scrolled: scrolledOf(element),
        last: kept.last,
        rowAtTop: () => rowAt(element, list),
        rowTop: (key) => topOfRow(element, list, key),
    });
    const wanted = placing.top === "bottom" ? element.scrollHeight : placing.top;
    kept.spot = placing.spot;
    kept.last = wanted === null ? element.scrollTop : put(element, wanted);
}

/** Scrolls to `top`, and says where that came to, which the browser keeps within bounds. */
function put(element: HTMLElement, top: number): number {
    element.scrollTop = top;
    return element.scrollTop;
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

/** Where the row of the item `key` is now, or `null` when it is not drawn. */
function topOfRow(element: HTMLElement, list: Element, key: number): number | null {
    const row = list.querySelector(`:scope > [data-key="${key}"]`);
    return row ? contentTop(element, row) : null;
}

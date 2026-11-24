import { type PointerEvent as ReactPointerEvent, useCallback } from "react";
import { dropGap, dropMove, type OrderTarget, targetOf } from "../ordering";

/** How far the pointer goes before a press on a row becomes a drag, in pixels. */
const DRAG_AFTER = 5;

/** A row being dragged among its siblings. */
interface Drag {
    row: HTMLElement;
    target: OrderTarget;
    siblings: HTMLElement[];
    /** Its place among them. */
    from: number;
    start: { x: number; y: number };
    moving: boolean;
    /** Where it would be dropped now: a gap among the siblings, or nowhere. */
    gap: number | null;
    cancelled: boolean;
}

/** The drag a press on a row's handle starts, if it is one. */
function dragFrom(event: ReactPointerEvent<HTMLElement>): Drag | null {
    const handle = (event.target as Element).closest("[data-drag-handle]");
    const row = handle?.closest<HTMLElement>("[data-order]");
    const target = row ? targetOf(row) : null;
    if (event.button !== 0 || !row || !target) {
        return null;
    }
    const group = row.getAttribute("data-order-group") ?? "";
    const siblings = [
        ...event.currentTarget.querySelectorAll<HTMLElement>("[data-order-group]"),
    ].filter((element) => element.getAttribute("data-order-group") === group);
    const start = { x: event.clientX, y: event.clientY };
    const from = siblings.indexOf(row);
    return { row, target, siblings, from, start, moving: false, gap: null, cancelled: false };
}

/** Takes the marks of a drag off the page. */
function unmark(drag: Drag) {
    drag.row.removeAttribute("data-dragging");
    for (const sibling of drag.siblings) {
        sibling.removeAttribute("data-drop");
    }
    document.documentElement.removeAttribute("data-reordering");
}

/** Shows where the row would land: a line before a sibling, or after the last. */
function mark(drag: Drag) {
    unmark(drag);
    drag.row.setAttribute("data-dragging", "");
    document.documentElement.setAttribute("data-reordering", "");
    const { gap, siblings } = drag;
    if (gap === null) {
        return;
    }
    const before = siblings[gap];
    if (before) {
        before.setAttribute("data-drop", "before");
    } else {
        siblings.at(-1)?.setAttribute("data-drop", "after");
    }
}

/** Follows the pointer: a drag once it has gone far enough, then the gap it is over. */
function follow(drag: Drag, event: PointerEvent) {
    const far = Math.hypot(event.clientX - drag.start.x, event.clientY - drag.start.y);
    if (drag.cancelled || (!drag.moving && far < DRAG_AFTER)) {
        return;
    }
    drag.moving = true;
    const rects = drag.siblings.map((sibling) => sibling.getBoundingClientRect());
    drag.gap = dropGap(event.clientY, rects);
    mark(drag);
}

/** Keeps the click that ends a drag from also pressing what it ended on. */
function swallowClick() {
    const swallow = (event: MouseEvent) => {
        event.stopPropagation();
        event.preventDefault();
    };
    window.addEventListener("click", swallow, { capture: true, once: true });
    // The click, if one comes, comes with the pointer's release; nothing later is swallowed.
    setTimeout(() => window.removeEventListener("click", swallow, { capture: true }), 0);
}

/** Lets go of a drag that is under way on Escape, and only that hears the key. */
function cancelOnEscape(drag: Drag) {
    return (pressed: KeyboardEvent) => {
        if (pressed.key === "Escape" && drag.moving) {
            pressed.stopPropagation();
            drag.cancelled = true;
            unmark(drag);
        }
    };
}

/** Ends a drag: the row is moved if it was dropped among its siblings. */
function finish(drag: Drag, onMove: (target: OrderTarget, by: number) => void) {
    unmark(drag);
    if (!drag.moving) {
        return;
    }
    swallowClick();
    const by = drag.gap === null || drag.cancelled ? 0 : dropMove(drag.from, drag.gap);
    if (by !== 0) {
        onMove(drag.target, by);
    }
}

/** Follows a drag from the press that may start it to the pointer's release. */
function track(drag: Drag, onMove: (target: OrderTarget, by: number) => void) {
    const move = (moved: PointerEvent) => follow(drag, moved);
    const key = cancelOnEscape(drag);
    const end = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", end);
        window.removeEventListener("pointercancel", end);
        window.removeEventListener("keydown", key, true);
        finish(drag, onMove);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
    window.addEventListener("keydown", key, true);
}

/**
 * Lets the rows of the sidebar be dragged among their siblings, from their handles: a
 * repository's heading, and the grip of a task, a session or a cloud task. A press that moves
 * the pointer a little starts it, a line shows where the row would go, and letting go there
 * moves it. Only the gaps among its siblings take it; Escape puts it back. Pointer events, not
 * the browser's drag and drop, which the window uses for dropping files to attach.
 */
export function useRowDrag(onMove: (target: OrderTarget, by: number) => void) {
    return useCallback(
        (event: ReactPointerEvent<HTMLElement>) => {
            const drag = dragFrom(event);
            if (drag) {
                // A press on a handle may become a drag, never a selection: WebKit begins one
                // before the pointer has gone far enough for the drag to turn selecting off.
                event.preventDefault();
                track(drag, onMove);
            }
        },
        [onMove],
    );
}

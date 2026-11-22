import type { Api } from "./api/client";
import type { Session } from "./api/types";

/**
 * A row of the sidebar the user can move among its siblings: a repository, a task among its
 * repository's open tasks, a session among its task's (or among those its starter started),
 * or a cloud task among its repository's. Sub-agents are never moved: they are not stored.
 */
export type OrderTarget =
    | { kind: "project"; project: string }
    | { kind: "task"; project: string; task: string }
    | { kind: "session"; id: number }
    | { kind: "cloud"; id: number };

/** What marks the siblings a row moves among, one name per list of the sidebar. */
export const orderGroup = {
    projects: () => "projects",
    tasks: (project: string) => `tasks:${project}`,
    /** A task's sessions, or with `starter` those that session started. */
    sessions: (task: number, starter: number | null) => `sessions:${task}:${starter ?? "top"}`,
    cloud: (project: string) => `cloud:${project}`,
};

/** The attributes that make an element a row that moves, and say among which siblings. */
export function orderAttributes(target: OrderTarget, group: string) {
    return { "data-order": JSON.stringify(target), "data-order-group": group };
}

/** The row an element of the sidebar stands for, read back from its attributes. */
export function targetOf(element: Pick<Element, "getAttribute">): OrderTarget | null {
    const written = element.getAttribute("data-order");
    if (written === null) {
        return null;
    }
    try {
        return JSON.parse(written) as OrderTarget;
    } catch {
        return null;
    }
}

/** The session that started `session` if it is listed in the same task, as `nestedSessions` has it. */
export function starterHere(sessions: Session[], session: Session): number | null {
    const starter = session.spawned_by;
    return starter !== null && sessions.some((other) => other.id === starter) ? starter : null;
}

/**
 * How many places a row moves when it is dropped in gap `gap`, where 0 is before the first of
 * its siblings and their count is after the last, from place `from` among them.
 */
export function dropMove(from: number, gap: number): number {
    return (gap > from ? gap - 1 : gap) - from;
}

/** How far outside its siblings a row may be dropped and still land among them, in pixels. */
const DROP_REACH = 12;

/**
 * The gap a row dragged to height `y` would be dropped in among siblings drawn at `rects`, in
 * order: before the first whose middle is below `y`, or after the last. `null` away from them.
 */
export function dropGap(y: number, rects: { top: number; bottom: number }[]): number | null {
    const first = rects[0];
    const last = rects.at(-1);
    if (!(first && last) || y < first.top - DROP_REACH || y > last.bottom + DROP_REACH) {
        return null;
    }
    const before = rects.findIndex((rect) => y < (rect.top + rect.bottom) / 2);
    return before === -1 ? rects.length : before;
}

/**
 * `items` with the one `moving` picks moved `by` places among those `among` picks, earlier for
 * a negative number and no further than either end. The others keep their places.
 */
export function movedAmong<T>(
    items: T[],
    among: (item: T) => boolean,
    moving: (item: T) => boolean,
    by: number,
): T[] {
    const slots = items.flatMap((item, index) => (among(item) ? [index] : []));
    const siblings = slots.flatMap((slot) => items.slice(slot, slot + 1));
    const from = siblings.findIndex(moving);
    if (from === -1) {
        return items;
    }
    const to = Math.min(siblings.length - 1, Math.max(0, from + by));
    const reordered = siblings.filter((_, index) => index !== from);
    reordered.splice(to, 0, ...siblings.slice(from, from + 1));
    const moved = [...items];
    slots.forEach((slot, index) => {
        const item = reordered[index];
        if (item !== undefined) {
            moved[slot] = item;
        }
    });
    return moved;
}

/** Asks the core to move `target` `by` places among its siblings. */
export function moveTarget(api: Api, target: OrderTarget, by: number): Promise<void> {
    switch (target.kind) {
        case "project":
            return api.projectMove(target.project, by);
        case "task":
            return api.taskMove(target.project, target.task, by);
        case "session":
            return api.sessionMove(target.id, by);
        case "cloud":
            return api.cloudTaskMove(target.id, by);
    }
}

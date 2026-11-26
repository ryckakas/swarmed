import type { Api } from "./api/client";
import type { Project } from "./api/types";
import { writtenHotkey } from "./hotkeys";
import { findSession } from "./tree";

/**
 * A row of the sidebar the user can move among its siblings: a repository, a task among its
 * repository's open tasks, a session among its task's (or among the others that work for its
 * parent, where it is listed under that one), or a cloud task among its repository's.
 * Sub-agents are never moved: they are not stored.
 */
export type OrderTarget =
    | { kind: "project"; project: string }
    | { kind: "task"; project: string; task: string }
    | { kind: "session"; id: number }
    | { kind: "cloud"; id: number };

/**
 * A row of the sidebar the user can pin above its siblings that are not pinned: a repository,
 * a task or a session. Cloud tasks are not pinned.
 */
export type PinTarget = Exclude<OrderTarget, { kind: "cloud" }>;

/** The siblings pinned as a row is: a pinned row moves among the pinned only, as the core has it. */
const pinGroup = (group: string, pinned: boolean) => (pinned ? `${group}:pinned` : group);

/** What marks the siblings a row moves among, one name per list of the sidebar. */
export const orderGroup = {
    projects: (pinned: boolean) => pinGroup("projects", pinned),
    tasks: (project: string, pinned: boolean) => pinGroup(`tasks:${project}`, pinned),
    /** A task's sessions, or with `parent` those listed under that one, which work for it. */
    sessions: (task: number, parent: number | null, pinned: boolean) =>
        pinGroup(`sessions:${task}:${parent ?? "top"}`, pinned),
    cloud: (project: string) => `cloud:${project}`,
};

/** What the grip a row is dragged by says under the pointer, with the keys that move a row too. */
export function gripTitle(): string {
    const up = writtenHotkey("move-up");
    const down = writtenHotkey("move-down");
    return `Drag to move. ${up} and ${down} move the focused row, or the open session.`;
}

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

/**
 * `items` with the one `pinning` picks pinned, put last among the pinned ones, or unpinned, put
 * first among the others, as the core does. The pinned ones are first in `items` already.
 */
export function pinnedAmong<T extends { pinned: boolean }>(
    items: T[],
    pinning: (item: T) => boolean,
    pinned: boolean,
): T[] {
    const item = items.find(pinning);
    if (item === undefined || item.pinned === pinned) {
        return items;
    }
    const others = items.filter((one) => one !== item);
    return [
        ...others.filter((one) => one.pinned),
        { ...item, pinned },
        ...others.filter((one) => !one.pinned),
    ];
}

/** Whether a row can be pinned: everything that moves but a cloud task. */
export function pinnable(target: OrderTarget): target is PinTarget {
    return target.kind !== "cloud";
}

/** Whether the row `target` stands for is pinned now; `null` if it is not in the tree. */
export function pinnedNow(projects: Project[], target: PinTarget): boolean | null {
    if (target.kind === "session") {
        return findSession(projects, target.id)?.pinned ?? null;
    }
    const project = projects.find((one) => one.name === target.project);
    if (target.kind === "project") {
        return project?.pinned ?? null;
    }
    const task = project?.tasks.find((one) => one.name === target.task && !one.archived);
    return task?.pinned ?? null;
}

/** What the pin on a row says under the pointer: what pressing it does, and the key that does the same. */
export function pinTitle(pinned: boolean): string {
    const keys = writtenHotkey("pin");
    return pinned
        ? `Pinned above the rest. Press to unpin, or ${keys} on the focused row.`
        : `Pin above the rest. ${keys} pins the focused row, or the open session.`;
}

/** What the pin on row `name` is called, for those who cannot see it. */
export function pinLabel(pinned: boolean, name: string): string {
    return pinned ? `Unpin ${name}` : `Pin ${name}`;
}

/** Asks the core to pin `target` above its siblings that are not pinned, or to unpin it. */
export function pinTarget(api: Api, target: PinTarget, pinned: boolean): Promise<void> {
    switch (target.kind) {
        case "project":
            return api.projectPin(target.project, pinned);
        case "task":
            return api.taskPin(target.project, target.task, pinned);
        case "session":
            return api.sessionPin(target.id, pinned);
    }
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

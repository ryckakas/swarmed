// What a clean-up lists and deletes, put into words, and which of it is ticked. Pure, so it
// can be tested.

import type { Cleaned, CleanupItem, CleanupKind } from "./api/types";
import { formatBytes } from "./usage";
import { capitalized, type Words } from "./wording";

/** Every kind, in the order the settings list them. */
export const CLEANUP_KINDS: CleanupKind[] = ["removed", "branches", "stale", "snapshots"];

/** How a kind is named in its line, and in the dialog that asks before deleting it. */
export interface KindWording {
    label: string;
    detail: string;
    /** The dialog's heading. */
    title: string;
    /** What the dialog says deleting them means. */
    what: string;
}

const FOR_GOOD = "Deleting cannot be undone.";

/** A kind's line and dialog, in the style's words. */
export function kindWording(kind: CleanupKind, words: Words): KindWording {
    const { task, tasks, worktrees, branches } = words.terms;
    switch (kind) {
        case "removed":
            return {
                label: `Removed ${worktrees}`,
                detail: "not deleted yet, or that could not be",
                title: `Delete removed ${worktrees}?`,
                what: `These are what was left of ${tasks} you removed. ${FOR_GOOD}`,
            };
        case "branches":
            return {
                label: `${capitalized(branches)} of removed ${tasks}`,
                detail: `kept when their ${tasks} were removed`,
                title: `Delete ${branches} of removed ${tasks}?`,
                what: `Swarmed made these for ${tasks} you removed. They are deleted here, with git branch -D; a copy on a remote stays. ${FOR_GOOD}`,
            };
        case "stale":
            return {
                label: `Stale ${worktrees}`,
                detail: `that no ${task} owns`,
                title: `Delete stale ${worktrees}?`,
                what: `Folders beside your ${worktrees} that no ${task} owns, and git's records of ${worktrees} whose folder is gone. ${capitalized(branches)} stay as they are. ${FOR_GOOD}`,
            };
        case "snapshots":
            return {
                label: words.snapshots,
                detail: `what new ${tasks} start from`,
                title: `Forget ${words.snapshots.toLowerCase()}?`,
                what: `Without one, a new ${task}'s setup does all the work again, and the first one set up well is kept in its place. Sizes count what may be shared with the ${worktrees} started from them, so less may be freed.`,
            };
    }
}

/** What a kind's line says of it: how many, and their size where they are on disk. */
export function kindSummary(items: CleanupItem[]): string {
    if (items.length === 0) {
        return "None";
    }
    const sizes = items.flatMap((item) => (item.bytes === null ? [] : [item.bytes]));
    if (sizes.length === 0) {
        return `${items.length}`;
    }
    return `${items.length}, ${formatBytes(sizes.reduce((sum, bytes) => sum + bytes, 0))}`;
}

/** What is ticked when the dialog opens: everything that loses no work. */
export function initialTicks(items: CleanupItem[]): string[] {
    return items.filter((item) => !item.at_risk).map((item) => item.key);
}

/** The ticks with `key` ticked, or unticked if it was. */
export function toggledTick(ticks: string[], key: string): string[] {
    return ticks.includes(key) ? ticks.filter((one) => one !== key) : [...ticks, key];
}

/** The keys of what is listed and ticked, in the list's order: what deleting is asked for. */
export function tickedKeys(items: CleanupItem[], ticks: string[]): string[] {
    return items.filter((item) => ticks.includes(item.key)).map((item) => item.key);
}

/** What the dialog's button says. */
export function deleteLabel(count: number, busy: boolean): string {
    if (busy) {
        return "Deleting…";
    }
    return count === 0 ? "Delete" : `Delete ${count}`;
}

/** What could not be deleted, each with why; `null` when everything asked for went. */
export function keptNote(cleaned: Cleaned): string | null {
    if (cleaned.kept.length === 0) {
        return null;
    }
    const each = cleaned.kept.map((kept) => `${kept.name} (${kept.reason})`);
    return `Not deleted: ${each.join("; ")}.`;
}

/** What an item is called in the dialog's list. */
export function itemTitle(kind: CleanupKind, item: CleanupItem): string {
    switch (kind) {
        case "removed":
            return `${item.project} / ${item.task ?? item.name}`;
        case "snapshots":
            return `${item.project} / ${item.name}`;
        default:
            return item.name;
    }
}

/** What is said under an item: why it is there, since when, and its size. */
export function itemDetail(kind: CleanupKind, item: CleanupItem, now: number): string {
    const since = item.since === null ? null : `${SINCE[kind]} ${age(item.since, now)}`;
    const size = item.bytes === null ? null : formatBytes(item.bytes);
    const parts = [origin(kind, item), item.reason, since, size];
    return parts.filter((part) => part !== null).join(" · ");
}

/** What `since` means for each kind. */
const SINCE: Record<CleanupKind, string> = {
    removed: "removed",
    branches: "removed",
    stale: "changed",
    snapshots: "taken",
};

/** The task an item was, or was taken from. */
function origin(kind: CleanupKind, item: CleanupItem): string | null {
    if (item.task === null || kind === "removed") {
        return null;
    }
    return kind === "snapshots" ? `from ${item.task}` : `of ${item.task}`;
}

const DAY = 86_400;

/** How long ago a moment was, in seconds since 1970, as people say it. */
export function age(since: number, now: number): string {
    const days = Math.floor(Math.max(0, now - since) / DAY);
    if (days === 0) {
        return "today";
    }
    if (days === 1) {
        return "yesterday";
    }
    const [count, unit] = ageIn(days);
    return `${count} ${unit}${count === 1 ? "" : "s"} ago`;
}

/** A number of days, in the largest unit that keeps it more than one. */
function ageIn(days: number): [number, string] {
    if (days < 14) {
        return [days, "day"];
    }
    if (days < 60) {
        return [Math.floor(days / 7), "week"];
    }
    if (days < 730) {
        return [Math.floor(days / 30), "month"];
    }
    return [Math.floor(days / 365), "year"];
}

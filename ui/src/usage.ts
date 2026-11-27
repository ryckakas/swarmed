// What Swarmed takes up, put into words. Pure, so it can be tested.

import type { Usage } from "./api/types";

const UNITS = ["kB", "MB", "GB", "TB"];

/** A size as people say it: 512 B, 3.4 MB, 1.2 GB. */
/** An amount of US dollars: cents for what is small, whole dollars above a hundred. */
export function formatUsd(usd: number): string {
    if (usd > 0 && usd < 0.01) {
        return "under $0.01";
    }
    return usd >= 100 ? `$${Math.round(usd).toLocaleString("en-US")}` : `$${usd.toFixed(2)}`;
}

export function formatBytes(bytes: number): string {
    let value = bytes;
    let unit = "B";
    for (const next of UNITS) {
        if (value < 1000) {
            break;
        }
        value /= 1000;
        unit = next;
    }
    if (unit === "B") {
        return `${bytes} B`;
    }
    return `${value >= 100 ? Math.round(value) : value.toFixed(1)} ${unit}`;
}

/**
 * Everything Swarmed keeps on disk: its database, the notes, the sessions' images, the turns'
 * checkpoints, the tasks' browsers, the built-in repolore's folder, every measured worktree,
 * and what is left of removed ones.
 */
export function totalBytes(usage: Usage): number {
    const worktrees = usage.worktrees.reduce((sum, worktree) => sum + (worktree.bytes ?? 0), 0);
    const kept =
        usage.database_bytes +
        usage.notes_bytes +
        usage.images_bytes +
        usage.checkpoints_bytes +
        usage.browsers_bytes +
        usage.repolore_bytes;
    return kept + worktrees + usage.removed_bytes;
}

/** Processor use as a share of one core: 0.4%, 12%, 180%. */
export function formatCpu(percent: number): string {
    return `${percent < 10 ? percent.toFixed(1) : Math.round(percent)}%`;
}

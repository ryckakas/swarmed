import type { Snapshot, Task } from "./api/types";

// Which snapshot goes with which environment, and what one can be taken from. Pure.

/** One line of the list: an environment, or the tasks without one, and its snapshot if any. */
export interface SnapshotRow {
    /** `null` for tasks without an environment. */
    environment: string | null;
    snapshot: Snapshot | null;
    /** The tasks a snapshot can be taken from: set up well, and in this environment. */
    sources: string[];
}

/** A row for the tasks without an environment, then one for each environment, in order. */
export function snapshotRows(
    environments: string[],
    snapshots: Snapshot[],
    tasks: Task[],
): SnapshotRow[] {
    const row = (environment: string | null): SnapshotRow => ({
        environment,
        snapshot: snapshots.find((one) => one.environment === environment) ?? null,
        sources: tasks
            .filter((task) => !task.archived && task.setup === "ok")
            .filter((task) => task.environment === environment)
            .map((task) => task.name),
    });
    return [row(null), ...environments.map(row)];
}

/** When a snapshot was taken, as a date and a time of day. */
export function takenAt(snapshot: Snapshot): string {
    return new Date(snapshot.taken_at * 1000).toLocaleString(undefined, {
        dateStyle: "medium",
        timeStyle: "short",
    });
}

// Tasks handed to cloud agents, as the sidebar shows them. Pure, so it can be tested.

import type { CloudTask } from "./api/types";

/** The statuses a provider gives a task that is not finished yet. */
const UNFINISHED = new Set(["pending", "in_progress", "running", "queued"]);

/** Whether the task may still change, so its status is worth asking for again. */
export function isUnfinished(task: CloudTask): boolean {
    return UNFINISHED.has(task.status);
}

/** What a task is called in a list: the first line of what it was asked to do. */
export function cloudTitle(task: CloudTask): string {
    return task.prompt.split("\n")[0]?.trim() || "Cloud task";
}

/** How the task's state is drawn: still going, done, or gone wrong. */
export function cloudTone(task: CloudTask): "going" | "done" | "failed" {
    if (isUnfinished(task)) {
        return "going";
    }
    return task.status === "error" || task.status === "failed" ? "failed" : "done";
}

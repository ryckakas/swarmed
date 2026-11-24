// What the task panel's Background list offers for each thing an agent left running. Pure, so
// it can be tested.

import type { BackgroundTask } from "./api/types";

/**
 * How a thing left running can be stopped from its row: with a button, while it runs and its
 * agent has a way to stop it; with a word that it cannot be, while it runs and its agent has
 * none, such as an OpenCode shell; or not at all, once it has ended.
 */
export function stopOffered(task: BackgroundTask): "button" | "cannot" | null {
    if (task.state !== "running") {
        return null;
    }
    return task.stoppable ? "button" : "cannot";
}

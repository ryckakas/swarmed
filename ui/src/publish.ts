// What is left to do before a task's work is out of its worktree, one step at a time.
// Pure, so it can be tested.

import type { Unpublished } from "./api/types";

/** The next thing to do with a task's work; `done` when nothing is left. */
export type PublishStep = "commit" | "push" | "pull" | "done";

/** Commit before pushing, push before a pull request, and one pull request is enough. */
export function nextStep(state: Unpublished, hasPull: boolean): PublishStep {
    if (state.changed > 0) {
        return "commit";
    }
    if (state.unpushed > 0) {
        return "push";
    }
    return state.pushed_before && !hasPull ? "pull" : "done";
}

const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** What has not left the worktree, in words. */
export function unpublishedSummary(state: Unpublished): string {
    const parts: string[] = [];
    if (state.changed > 0) {
        parts.push(`${count(state.changed, "file", "files")} not committed`);
    }
    if (state.unpushed > 0) {
        parts.push(`${count(state.unpushed, "commit", "commits")} not pushed`);
    }
    if (parts.length > 0) {
        return parts.join(", ");
    }
    return state.pushed_before ? "Everything is committed and pushed" : "Nothing to publish yet";
}

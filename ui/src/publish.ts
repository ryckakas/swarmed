// What is left to do before a task's work is out of its worktree, one step at a time.
// Pure, so it can be tested.

import type { Unpublished } from "./api/types";
import { type Words, wordsFor } from "./wording";

/** The next thing to do with a task's work; `done` when nothing is left. */
export type PublishStep = "commit" | "push" | "pull" | "done";

/** What the panel has to say about the branch a task's worktree is on, if anything. */
export type BranchNotice = "rebasing" | "detached" | "several";

/**
 * A rebase under way, a worktree on no branch, or one that has been on more than one branch:
 * each is said, since what is counted, pushed and shown is then not simply the task's own.
 */
export function branchNotice(state: Unpublished, taskBranch: string): BranchNotice | null {
    if (state.rebasing) {
        return "rebasing";
    }
    if (state.branch === null) {
        return "detached";
    }
    return state.branches.length > 1 || state.branch !== taskBranch ? "several" : null;
}

/**
 * Commit before pushing, push before a pull request, and one pull request is enough. All of
 * it is of the branch the worktree is on; on no branch only committing is left, since there
 * is nothing to push or open a pull request for.
 */
export function nextStep(state: Unpublished, hasPull: boolean): PublishStep {
    if (state.changed > 0) {
        return "commit";
    }
    if (state.branch === null) {
        return "done";
    }
    if (state.unpushed > 0) {
        return "push";
    }
    return state.pushed_before && !hasPull ? "pull" : "done";
}

const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** What has not left the worktree, in the style's words. */
export function unpublishedSummary(state: Unpublished, words: Words = wordsFor("plain")): string {
    const { commit, commits, committed, pushed } = words.terms;
    const parts: string[] = [];
    if (state.changed > 0) {
        parts.push(`${count(state.changed, "file", "files")} not ${committed}`);
    }
    if (state.unpushed > 0) {
        parts.push(`${count(state.unpushed, commit, commits)} not ${pushed}`);
    }
    if (parts.length > 0) {
        return parts.join(", ");
    }
    return state.pushed_before
        ? `Everything is ${committed} and ${pushed}`
        : "Nothing to publish yet";
}

/**
 * What is said above the choice of branch: where the worktree is, and that what is counted,
 * committed, pushed and shown as a pull request is that branch's and no other's.
 */
export function branchesSaid(
    state: Unpublished,
    taskBranch: string,
    words: Words = wordsFor("plain"),
): string {
    const { branch, commit, push, pullRequest, task, worktree } = words.terms;
    const carried = `Choosing another takes what is not ${words.terms.committed} along, unless it conflicts.`;
    if (state.branch === null) {
        return `This ${worktree} is on no ${branch}, so there is nothing to ${push}. A ${commit} made here belongs to no ${branch}: choose one first. ${carried}`;
    }
    const own =
        state.branch === taskBranch ? "" : `, not on this ${task}'s own ${branch} ${taskBranch}`;
    return `This ${worktree} has been on more than one ${branch}. It is on ${state.branch} now${own}: the changes, what is counted here and the ${pullRequest} are that ${branch}'s, and ${commit} and ${push} go to it. ${carried}`;
}

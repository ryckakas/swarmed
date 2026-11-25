// What the merge dialog offers and says, for a task's pull request. Pure, so it can be tested.

import type { MergeMethod, MergeOutcome, MergeRules, MergeWhen, Pull } from "./api/types";
import { failedChecks } from "./pulls";

/** A way to merge now, as the split button's menu offers them. */
export type MergeWay = Exclude<MergeWhen, "auto">;

/** What each way is called in the split button's menu. */
export const WAY_NAMES: Record<MergeWay, string> = {
    now: "Merge",
    force: "Force merge",
    bypass: "Bypass requirements",
};

/** What the main button says for each way: what it does. */
const WAY_BUTTONS: Record<MergeWay, string> = {
    now: "Merge",
    force: "Force merge",
    bypass: "Bypass requirements and merge",
};

/** What each method is called in the dialog's menu of them. */
export const METHOD_NAMES: Record<MergeMethod, string> = {
    merge: "Merge commit",
    squash: "Squash",
    rebase: "Rebase",
};

/** How each method puts the commits into the base, in words. */
const METHOD_SAID: Record<MergeMethod, string> = {
    merge: "with a merge commit",
    squash: "as one squashed commit",
    rebase: "by rebasing its commits",
};

/** Whether a pull request can be merged from the window: open, not a draft, and read whole. */
export function mergeable(pull: Pull): boolean {
    return pull.state === "open" && !pull.draft && pull.head !== "";
}

/** The ways offered: past the requirements only to the repository's admins. */
export function mergeWays(rules: MergeRules): MergeWay[] {
    return rules.admin ? ["now", "force", "bypass"] : ["now", "force"];
}

/** What the main button says. */
export function mergeButton(way: MergeWay): string {
    return WAY_BUTTONS[way];
}

/** The first characters of a commit's id, as one names it. */
export function shortHead(head: string): string {
    return head.slice(0, 7);
}

/** What pressing the main button does, in one sentence: "Merges #412 into main as one…". */
export function mergeSaid(pull: Pull, way: MergeWay, method: MergeMethod): string {
    const how = `Merges #${pull.number} into ${pull.base || "its base"} ${METHOD_SAID[method]}, at ${shortHead(pull.head)}`;
    switch (way) {
        case "now":
            return `${how}.`;
        case "force":
            return `${how}, although checks failed. GitHub still refuses where a rule requires them.`;
        case "bypass":
            return `${how}, past the repository's requirements, as its admin.`;
    }
}

/**
 * Why the main button is held back, if it is: a plain merge while checks failed points to a
 * forced one instead of asking again.
 */
export function heldBack(pull: Pull, way: MergeWay): string | null {
    const failed = failedChecks(pull);
    if (way !== "now" || failed.length === 0) {
        return null;
    }
    const which = failed.length === 1 ? "A check failed" : `${failed.length} checks failed`;
    return `${which}. Choose ${WAY_NAMES.force} to merge anyway, where no rule of the repository requires ${failed.length === 1 ? "it" : "them"}.`;
}

/** Whether GitHub would merge it now, so that auto-merge would merge it at once. */
export function mergeableNow(pull: Pull): boolean {
    return ["clean", "unstable", "has_hooks"].includes(pull.merge_state ?? "");
}

/** How GitHub's requirements stand, in words, if GitHub has said. */
export function requirementsSaid(pull: Pull): string | null {
    switch (pull.merge_state) {
        case "clean":
        case "has_hooks":
            return "Its requirements are met.";
        case "unstable":
            return "Its requirements are met; checks no rule requires did not pass.";
        case "blocked":
            return "A requirement of the repository is not met yet.";
        case "behind":
            return "Its base moved on, and the repository wants it up to date first.";
        case "dirty":
            return "It conflicts with its base.";
        default:
            return null;
    }
}

/** The auto-merge choice: on, available, or why not. */
export type AutoMergeChoice =
    | { kind: "on"; method: MergeMethod }
    | { kind: "available" }
    | { kind: "unavailable"; why: string };

/** Whether auto-merge can be turned on, is on, or why it cannot be. */
export function autoMergeChoice(pull: Pull, rules: MergeRules): AutoMergeChoice {
    if (pull.auto_merge !== null) {
        return { kind: "on", method: pull.auto_merge };
    }
    if (!rules.auto_merge) {
        return { kind: "unavailable", why: "This repository does not allow auto-merge." };
    }
    if (mergeableNow(pull)) {
        return {
            kind: "unavailable",
            why: "Its requirements are met, so auto-merge would merge it at once: merge it instead.",
        };
    }
    return { kind: "available" };
}

/** What came of a merge, in one sentence. */
export function outcomeSaid(outcome: MergeOutcome, pull: Pull, method: MergeMethod): string {
    const base = pull.base || "its base";
    switch (outcome) {
        case "merged":
            return `#${pull.number} is merged into ${base}.`;
        case "queued":
            return `#${pull.number} is in the merge queue of ${base}, which merges it once the queue's checks pass.`;
        case "auto_merge":
            return `Auto-merge is on: GitHub merges #${pull.number} ${METHOD_SAID[method]} once its requirements are met.`;
        case "auto_merge_off":
            return `Auto-merge of #${pull.number} is off.`;
        case "asked":
            return `GitHub took the merge of #${pull.number}. What came of it shows once it is read again.`;
    }
}

/**
 * What GitHub said when it did not merge, with the flags `gh` names as what they are in the
 * window.
 */
export function mergeProblem(said: string): string {
    return said
        .replace(/add the `--auto` flag/g, "turn auto-merge on")
        .replace(/add the `--admin` flag/g, `choose ${WAY_NAMES.bypass}`);
}

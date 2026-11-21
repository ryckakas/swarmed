// What a task's pull request and its checks come to, for the sidebar and the task panel.
// Pure, so it can be tested.

import type { Check, Pull } from "./api/types";

/** How a pull request's checks stand as a whole. */
export type PullTone = "failed" | "running" | "passed" | "none";

/** One failed check outweighs the rest; one still running outweighs those that passed. */
export function pullTone(pull: Pull): PullTone {
    const has = (outcome: Check["outcome"]) => pull.checks.some((c) => c.outcome === outcome);
    if (has("failed")) {
        return "failed";
    }
    if (has("running")) {
        return "running";
    }
    return has("passed") ? "passed" : "none";
}

/**
 * Whether the list of checks is shown before the user says: it is while something failed or
 * is still running, which is when each check matters, and folded away once all is well.
 */
export function checksShownByDefault(pull: Pull): boolean {
    const tone = pullTone(pull);
    return tone === "failed" || tone === "running";
}

const STATES: Record<Pull["state"], string> = { open: "open", merged: "merged", closed: "closed" };

/** Where the pull request stands, in a word or two. */
export function pullState(pull: Pull): string {
    return pull.draft && pull.state === "open" ? "draft" : STATES[pull.state];
}

/** How the checks stand, in words: "3 of 5 checks passed, 1 failed, 1 running". */
export function checksSummary(pull: Pull): string {
    // Skipped checks are neither good nor bad, and say nothing about the change.
    const counted = pull.checks.filter((check) => check.outcome !== "skipped");
    if (counted.length === 0) {
        return "no checks";
    }
    const count = (outcome: Check["outcome"]) =>
        counted.filter((check) => check.outcome === outcome).length;
    const noun = counted.length === 1 ? "check" : "checks";
    const parts = [`${count("passed")} of ${counted.length} ${noun} passed`];
    if (count("failed") > 0) {
        parts.push(`${count("failed")} failed`);
    }
    if (count("running") > 0) {
        parts.push(`${count("running")} running`);
    }
    return parts.join(", ");
}

/** What pointing at the pull request's badge says. */
export function pullTooltip(pull: Pull): string {
    return `Pull request #${pull.number}, ${pullState(pull)}: ${checksSummary(pull)}. Click to open.`;
}

/** The checks that failed. */
export function failedChecks(pull: Pull): Check[] {
    return pull.checks.filter((check) => check.outcome === "failed");
}

/** What to tell an agent so that it looks into the failed checks. Nothing if none failed. */
export function fixPrompt(pull: Pull): string {
    const failed = failedChecks(pull);
    if (failed.length === 0) {
        return "";
    }
    const lines = failed.map((check) => `- ${check.name}${check.url ? `: ${check.url}` : ""}`);
    return [
        `These checks failed on pull request #${pull.number} (${pull.url}):`,
        ...lines,
        "Find out why they fail (`gh run view --log-failed` shows a run's log), fix the cause in this worktree, and tell me what you changed. Do not push.",
    ].join("\n");
}

/** How long to wait before asking again, in milliseconds: sooner while checks are running. */
export function pullPollMs(pull: Pull | null | undefined): number {
    return pull && pull.state === "open" && pullTone(pull) === "running" ? 20_000 : 90_000;
}

/** The order checks are listed in: what needs looking at first. */
export const OUTCOME_ORDER: Check["outcome"][] = ["failed", "running", "passed", "skipped"];

/** How many checks there are of each outcome, for those that have any, in the order they are listed. */
export function checkCounts(checks: Check[]): [Check["outcome"], number][] {
    return OUTCOME_ORDER.map((outcome): [Check["outcome"], number] => [
        outcome,
        checks.filter((check) => check.outcome === outcome).length,
    ]).filter(([, count]) => count > 0);
}

/**
 * The checks to list: those of the outcome asked for, if any, that hold every word typed,
 * whatever its case, in their name or in the name of their workflow. Failed ones come first,
 * then running ones, each group in GitHub's order.
 */
export function listedChecks(
    checks: Check[],
    outcome: Check["outcome"] | null,
    typed: string,
): Check[] {
    const words = typed.toLowerCase().split(/\s+/).filter(Boolean);
    const holds = (check: Check) => {
        const said = `${check.workflow ?? ""} ${check.name}`.toLowerCase();
        return words.every((word) => said.includes(word));
    };
    const kept = checks.filter(
        (check) => (outcome === null || check.outcome === outcome) && holds(check),
    );
    const rank = (check: Check) => OUTCOME_ORDER.indexOf(check.outcome);
    // Sorting keeps the order of what ranks the same.
    return [...kept].sort((one, other) => rank(one) - rank(other));
}

/** From how many checks on the list gets a line to find one by name. */
export const CHECKS_SEARCHED_FROM = 8;

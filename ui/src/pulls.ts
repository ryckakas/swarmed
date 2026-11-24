// What a task's pull request and its checks come to, for the sidebar and the task panel.
// Pure, so it can be tested.

import type { Check, Project, Pull, PullChange } from "./api/types";
import { capitalized, type Words, wordsFor } from "./wording";

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

/** What pointing at the pull request's badge says, in the style's words. */
export function pullTooltip(pull: Pull, words: Words = wordsFor("plain")): string {
    const name = capitalized(words.terms.pullRequest);
    return `${name} #${pull.number}, ${pullState(pull)}: ${checksSummary(pull)}. Click to open.`;
}

/** The checks that failed. */
export function failedChecks(pull: Pull): Check[] {
    return pull.checks.filter((check) => check.outcome === "failed");
}

/**
 * Each task's pull request as the core last read it, by the task's id, from the tree. The core
 * follows them; the window only shows what it read.
 */
export function pullsOf(projects: Project[]): Record<number, Pull | null> {
    const entries = projects.flatMap((project) =>
        project.tasks.map((task): [number, Pull | null] => [task.id, task.pull ?? null]),
    );
    return Object.fromEntries(entries);
}

/** Names joined as one says them: "alice", "alice and bob", "alice, bob and carol". */
function namesSaid(names: string[]): string {
    const last = names.at(-1) ?? "someone";
    return names.length < 2 ? last : `${names.slice(0, -1).join(", ")} and ${last}`;
}

/**
 * What changed on a pull request, as one line of the transcript: which checks, or who. Never
 * what a review or comment says, which the core does not keep.
 */
export function pullChangeSaid(
    number: number,
    change: PullChange,
    words: Words = wordsFor("plain"),
): string {
    const pull = `${words.terms.pullRequest} #${number}`;
    switch (change.kind) {
        case "checks_failed":
            return `${change.checks.length === 1 ? "A check" : "Checks"} failed on ${pull}: ${change.checks.join(", ")}`;
        case "checks_passed":
            return `All checks passed on ${pull}`;
        case "changes_requested":
            return `${namesSaid(change.by)} asked for changes on ${pull}`;
        case "approved":
            return `${namesSaid(change.by)} approved ${pull}`;
        case "reviewed":
            return `${namesSaid(change.by)} reviewed ${pull}`;
        case "commented":
            return `${namesSaid(change.by)} commented on ${pull}`;
        case "conflicts":
            return `${capitalized(pull)} conflicts with its base`;
        case "merged":
            return `${capitalized(pull)} was merged`;
        case "closed":
            return `${capitalized(pull)} was closed`;
    }
}

/** What the message box holds once `message` is put in after what was typed, if anything. */
export function withMessage(typed: string, message: string): string {
    return typed.trim() === "" ? message : `${typed.trimEnd()}\n\n${message}`;
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

import { describe, expect, it } from "vitest";
import type { Check, MergeRules, Pull } from "./api/types";
import {
    autoMergeChoice,
    heldBack,
    mergeable,
    mergeButton,
    mergeProblem,
    mergeSaid,
    mergeWays,
    outcomeSaid,
    requirementsSaid,
} from "./merging";

const check = (name: string, outcome: Check["outcome"]): Check => ({
    name,
    workflow: null,
    outcome,
    url: null,
});

const pull = (more: Partial<Pull> = {}): Pull => ({
    number: 412,
    title: "Fix the login redirect",
    url: "https://github.com/o/r/pull/412",
    state: "open",
    draft: false,
    review: null,
    checks: [check("Lint", "passed")],
    conflicts: false,
    head: "0123456789abcdef",
    base: "main",
    auto_merge: null,
    merge_state: "blocked",
    merged_at: null,
    ...more,
});

const rules = (more: Partial<MergeRules> = {}): MergeRules => ({
    methods: ["merge", "squash"],
    method: "squash",
    auto_merge: true,
    admin: false,
    ...more,
});

describe("mergeable", () => {
    it("is an open pull request that is not a draft, read with its head", () => {
        expect(mergeable(pull())).toBe(true);
        expect(mergeable(pull({ draft: true }))).toBe(false);
        expect(mergeable(pull({ state: "merged" }))).toBe(false);
        expect(mergeable(pull({ state: "closed" }))).toBe(false);
        // Kept by a Swarmed that did not read heads: read again first.
        expect(mergeable(pull({ head: "" }))).toBe(false);
    });
});

describe("mergeWays", () => {
    it("offers bypassing the requirements to admins only", () => {
        expect(mergeWays(rules())).toEqual(["now", "force"]);
        expect(mergeWays(rules({ admin: true }))).toEqual(["now", "force", "bypass"]);
    });
});

describe("mergeSaid", () => {
    it("says what the main button does, with the method and the head", () => {
        expect(mergeButton("bypass")).toBe("Bypass requirements and merge");
        expect(mergeSaid(pull(), "now", "squash")).toBe(
            "Merges #412 into main as one squashed commit, at 0123456.",
        );
        expect(mergeSaid(pull(), "force", "merge")).toContain("although checks failed");
        expect(mergeSaid(pull(), "bypass", "rebase")).toContain(
            "past the repository's requirements",
        );
    });
});

describe("heldBack", () => {
    it("points a plain merge with failed checks to a forced one", () => {
        const failing = pull({ checks: [check("Test", "failed"), check("Lint", "passed")] });
        expect(heldBack(failing, "now")).toBe(
            "A check failed. Choose Force merge to merge anyway, where no rule of the repository requires it.",
        );
        expect(heldBack(failing, "force")).toBeNull();
        expect(heldBack(failing, "bypass")).toBeNull();
        expect(heldBack(pull({ checks: [check("Test", "running")] }), "now")).toBeNull();
        const two = pull({ checks: [check("A", "failed"), check("B", "failed")] });
        expect(heldBack(two, "now")).toContain("2 checks failed");
    });
});

describe("autoMergeChoice", () => {
    it("is on, available while requirements wait, or says why not", () => {
        expect(autoMergeChoice(pull({ auto_merge: "rebase" }), rules())).toEqual({
            kind: "on",
            method: "rebase",
        });
        expect(autoMergeChoice(pull(), rules())).toEqual({ kind: "available" });
        expect(autoMergeChoice(pull({ merge_state: null }), rules())).toEqual({
            kind: "available",
        });
        const refused = autoMergeChoice(pull(), rules({ auto_merge: false }));
        expect(refused).toEqual({
            kind: "unavailable",
            why: "This repository does not allow auto-merge.",
        });
        for (const state of ["clean", "unstable", "has_hooks"]) {
            const now = autoMergeChoice(pull({ merge_state: state }), rules());
            expect(now.kind).toBe("unavailable");
        }
    });
});

describe("requirementsSaid", () => {
    it("words what GitHub worked out, and nothing while it has not", () => {
        expect(requirementsSaid(pull())).toBe("A requirement of the repository is not met yet.");
        expect(requirementsSaid(pull({ merge_state: "clean" }))).toBe("Its requirements are met.");
        expect(requirementsSaid(pull({ merge_state: null }))).toBeNull();
    });
});

describe("outcomeSaid", () => {
    it("names a merge queue as one, and a merge as a merge", () => {
        expect(outcomeSaid("merged", pull(), "squash")).toBe("#412 is merged into main.");
        expect(outcomeSaid("queued", pull(), "squash")).toContain("merge queue of main");
        expect(outcomeSaid("auto_merge", pull(), "squash")).toBe(
            "Auto-merge is on: GitHub merges #412 as one squashed commit once its requirements are met.",
        );
    });
});

describe("mergeProblem", () => {
    it("names the flags gh suggests as what they are in the window", () => {
        const said =
            "#412 was not merged: Pull request o/r#412 is not mergeable: the base branch policy prohibits the merge.\nTo have the pull request merged after all the requirements have been met, add the `--auto` flag.\nTo use administrator privileges to immediately merge the pull request, add the `--admin` flag.";
        const problem = mergeProblem(said);
        expect(problem).toContain("have been met, turn auto-merge on.");
        expect(problem).toContain("merge the pull request, choose Bypass requirements.");
        expect(problem).not.toContain("--");
    });
});

import { describe, expect, it } from "vitest";
import type { Check, Pull } from "./api/types";
import {
    checkCounts,
    checksSummary,
    fixPrompt,
    listedChecks,
    pullPollMs,
    pullState,
    pullTone,
    pullTooltip,
} from "./pulls";

const check = (name: string, outcome: Check["outcome"], url: string | null = null): Check => ({
    name,
    workflow: null,
    outcome,
    url,
});
const pull = (checks: Check[], more: Partial<Pull> = {}): Pull => ({
    number: 12,
    title: "Fix it",
    url: "https://github.com/o/r/pull/12",
    state: "open",
    draft: false,
    review: null,
    checks,
    ...more,
});

describe("pullTone", () => {
    it("lets one failure outweigh everything, and one running check the passed ones", () => {
        const passed = check("a", "passed");
        expect(pullTone(pull([passed, check("b", "running"), check("c", "failed")]))).toBe(
            "failed",
        );
        expect(pullTone(pull([passed, check("b", "running")]))).toBe("running");
        expect(pullTone(pull([passed, check("b", "skipped")]))).toBe("passed");
        expect(pullTone(pull([]))).toBe("none");
        expect(pullTone(pull([check("b", "skipped")]))).toBe("none");
    });
});

describe("checksSummary", () => {
    it("counts what passed, failed and still runs, and leaves skipped checks out", () => {
        const checks = [
            check("a", "passed"),
            check("b", "passed"),
            check("c", "failed"),
            check("d", "running"),
            check("e", "skipped"),
        ];
        expect(checksSummary(pull(checks))).toBe("2 of 4 checks passed, 1 failed, 1 running");
        expect(checksSummary(pull([check("a", "passed")]))).toBe("1 of 1 check passed");
        expect(checksSummary(pull([]))).toBe("no checks");
    });
});

describe("pullState and pullTooltip", () => {
    it("calls an open draft a draft, and a merged one merged whatever it was", () => {
        expect(pullState(pull([], { draft: true }))).toBe("draft");
        expect(pullState(pull([], { draft: true, state: "merged" }))).toBe("merged");
        expect(pullTooltip(pull([check("a", "passed")]))).toBe(
            "Pull request #12, open: 1 of 1 check passed. Click to open.",
        );
    });
});

describe("fixPrompt", () => {
    it("names the failed checks with where to read about them", () => {
        const checks = [
            check("Test", "failed", "https://ci/1"),
            check("Lint", "failed"),
            check("Docs", "passed"),
        ];
        const prompt = fixPrompt(pull(checks));
        expect(prompt).toContain("pull request #12");
        expect(prompt).toContain("- Test: https://ci/1");
        expect(prompt).toContain("- Lint\n");
        expect(prompt).not.toContain("Docs");
        expect(prompt).toContain("Do not push");
    });

    it("is nothing when nothing failed", () => {
        expect(fixPrompt(pull([check("Docs", "passed")]))).toBe("");
    });
});

describe("pullPollMs", () => {
    it("asks sooner while checks run on an open pull request", () => {
        const running = pull([check("a", "running")]);
        expect(pullPollMs(running)).toBeLessThan(pullPollMs(pull([check("a", "passed")])));
        expect(pullPollMs({ ...running, state: "merged" })).toBe(pullPollMs(null));
    });
});

describe("the list of checks", () => {
    const check = (
        name: string,
        outcome: Check["outcome"],
        workflow: string | null = null,
    ): Check => ({
        name,
        workflow,
        outcome,
        url: null,
    });
    const checks = [
        check("Lint", "passed"),
        check("Unit tests", "failed"),
        check("Browser tests", "running"),
        check("Deploy preview", "skipped"),
        check("E2E tests", "failed"),
    ];
    const names = (listed: Check[]) => listed.map((one) => one.name);

    it("puts what failed first, then what runs, each as GitHub ordered them", () => {
        expect(names(listedChecks(checks, null, ""))).toEqual([
            "Unit tests",
            "E2E tests",
            "Browser tests",
            "Lint",
            "Deploy preview",
        ]);
    });

    it("narrows to one outcome, and to names that hold what was typed", () => {
        expect(names(listedChecks(checks, "failed", ""))).toEqual(["Unit tests", "E2E tests"]);
        expect(names(listedChecks(checks, null, " TESTS "))).toEqual([
            "Unit tests",
            "E2E tests",
            "Browser tests",
        ]);
        expect(names(listedChecks(checks, "failed", "e2e"))).toEqual(["E2E tests"]);
        expect(listedChecks(checks, "passed", "deploy")).toEqual([]);
    });

    it("counts each outcome there is, in the order they are listed", () => {
        expect(checkCounts(checks)).toEqual([
            ["failed", 2],
            ["running", 1],
            ["passed", 1],
            ["skipped", 1],
        ]);
        expect(checkCounts([check("Lint", "passed")])).toEqual([["passed", 1]]);
    });

    it("finds a job by the name of its workflow, and by several words in any order", () => {
        const jobs = [
            check("chromium (1/2)", "passed", "E2E"),
            check("chromium (2/2)", "failed", "E2E"),
            check("chromium", "passed", "Visual tests"),
            check("Lint", "passed", "CI"),
        ];
        expect(names(listedChecks(jobs, null, "e2e"))).toEqual([
            "chromium (2/2)",
            "chromium (1/2)",
        ]);
        expect(names(listedChecks(jobs, null, "chromium e2e 1/2"))).toEqual(["chromium (1/2)"]);
        expect(names(listedChecks(jobs, null, "visual"))).toEqual(["chromium"]);
        expect(listedChecks(jobs, null, "e2e lint")).toEqual([]);
    });
});

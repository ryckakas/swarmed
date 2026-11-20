import { describe, expect, it } from "vitest";
import type { Check, Pull } from "./api/types";
import { checksSummary, fixPrompt, pullPollMs, pullState, pullTone, pullTooltip } from "./pulls";

const check = (name: string, outcome: Check["outcome"], url: string | null = null): Check => ({
    name,
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

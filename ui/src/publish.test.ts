import { describe, expect, it } from "vitest";
import { nextStep, unpublishedSummary } from "./publish";
import { wordsFor } from "./wording";

const state = (changed: number, unpushed: number, pushed_before: boolean) => ({
    changed,
    unpushed,
    pushed_before,
});

describe("nextStep", () => {
    it("goes commit, push, pull request, done", () => {
        expect(nextStep(state(2, 1, true), false)).toBe("commit");
        expect(nextStep(state(0, 1, false), false)).toBe("push");
        expect(nextStep(state(0, 0, true), false)).toBe("pull");
        expect(nextStep(state(0, 0, true), true)).toBe("done");
    });

    it("offers no pull request for a branch that was never pushed", () => {
        expect(nextStep(state(0, 0, false), false)).toBe("done");
    });
});

describe("unpublishedSummary", () => {
    it("says what has not left the worktree", () => {
        expect(unpublishedSummary(state(1, 0, false))).toBe("1 file not committed");
        expect(unpublishedSummary(state(3, 2, true))).toBe(
            "3 files not committed, 2 commits not pushed",
        );
        expect(unpublishedSummary(state(0, 1, true))).toBe("1 commit not pushed");
    });

    it("tells a finished branch from one nothing happened on", () => {
        expect(unpublishedSummary(state(0, 0, true))).toBe("Everything is committed and pushed");
        expect(unpublishedSummary(state(0, 0, false))).toBe("Nothing to publish yet");
        const colony = wordsFor("colony");
        expect(unpublishedSummary(state(3, 2, true), colony)).toBe(
            "3 files not sealed, 2 seals not released",
        );
        expect(unpublishedSummary(state(0, 0, true), colony)).toBe(
            "Everything is sealed and released",
        );
    });
});

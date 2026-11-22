import { describe, expect, it } from "vitest";
import { branchesSaid, branchNotice, nextStep, unpublishedSummary } from "./publish";
import { wordsFor } from "./wording";

const OWN = "you/fix-login";

const state = (
    changed: number,
    unpushed: number,
    pushed_before: boolean,
    branch: string | null = OWN,
    branches: string[] = [OWN],
) => ({ changed, unpushed, pushed_before, branch, branches, rebasing: false });

describe("branchNotice", () => {
    it("says nothing while the worktree has only been on the task's branch", () => {
        expect(branchNotice(state(0, 1, true), OWN)).toBeNull();
    });

    it("says so once it has been on another, wherever it is now", () => {
        expect(branchNotice(state(0, 1, true, "second", [OWN, "second"]), OWN)).toBe("several");
        expect(branchNotice(state(0, 1, true, OWN, [OWN, "second"]), OWN)).toBe("several");
    });

    it("tells no branch from a rebase under way", () => {
        expect(branchNotice(state(0, 1, true, null), OWN)).toBe("detached");
        const rebasing = { ...state(0, 1, true, null), rebasing: true };
        expect(branchNotice(rebasing, OWN)).toBe("rebasing");
    });
});

describe("branchesSaid", () => {
    it("names the branch at hand, and the task's own when that is another", () => {
        const away = branchesSaid(state(0, 0, true, "second", [OWN, "second"]), OWN);
        expect(away).toContain("It is on second now, not on this task's own branch you/fix-login:");
        const back = branchesSaid(state(0, 0, true, OWN, [OWN, "second"]), OWN);
        expect(back).toContain("It is on you/fix-login now:");
    });

    it("says that on no branch there is nothing to push", () => {
        expect(branchesSaid(state(0, 0, true, null), OWN)).toContain("is on no branch");
    });
});

describe("nextStep", () => {
    it("goes commit, push, pull request, done", () => {
        expect(nextStep(state(2, 1, true), false)).toBe("commit");
        expect(nextStep(state(0, 1, false), false)).toBe("push");
        expect(nextStep(state(0, 0, true), false)).toBe("pull");
        expect(nextStep(state(0, 0, true), true)).toBe("done");
    });

    it("goes on as ever on another branch than the task's, and only commits on none", () => {
        expect(nextStep(state(0, 1, false, "second", [OWN, "second"]), false)).toBe("push");
        expect(nextStep(state(2, 1, true, null), false)).toBe("commit");
        expect(nextStep(state(0, 1, false, null), false)).toBe("done");
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

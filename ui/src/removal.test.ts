import { describe, expect, it } from "vitest";
import { anywayHelps, refusalSaid, removedWhileSetUp } from "./removal";

describe("a removal that was refused", () => {
    it("can go ahead anyway over unsaved work, which it discards", () => {
        const problem = "task `one` has uncommitted changes";
        expect(anywayHelps(problem)).toBe(true);
        expect(refusalSaid(problem)).toContain("Removing anyway discards that work.");
    });

    it("cannot over an agent another Swarmed runs, which only that one can stop", () => {
        const problem =
            "session 7 runs in another Swarmed: do this there, or once it is stopped there or that Swarmed has quit";
        expect(anywayHelps(problem)).toBe(false);
        expect(refusalSaid(problem)).toBe(`Not removed: ${problem}.`);
    });
});

describe("a task removed while it was set up", () => {
    it("is no failure of its setup", () => {
        expect(
            removedWhileSetUp("the setup of `one` was stopped because its task was removed"),
        ).toBe(true);
    });

    it("leaves a setup that failed, or was stopped otherwise, to be said", () => {
        expect(removedWhileSetUp("the `setup` hook failed with signal: 9 (SIGKILL)")).toBe(false);
        expect(removedWhileSetUp("the setup of `one` was stopped before it finished")).toBe(false);
    });
});

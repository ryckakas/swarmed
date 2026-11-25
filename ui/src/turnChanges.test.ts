import { describe, expect, it } from "vitest";
import type { Turn } from "./api/types";
import { choiceOf, chosenTurn, turnLabel, turnNotes, turnOptions, WHOLE } from "./turnChanges";

const turn = (message: number, extra: Partial<Turn> = {}): Turn => ({
    message,
    words: `message ${message}`,
    at_ms: 0,
    ended: true,
    beside: [],
    ...extra,
});

describe("chosenTurn", () => {
    const turns = [turn(0), turn(2)];

    it("is the turn chosen while it is one of the session on screen that is kept", () => {
        expect(chosenTurn({ session: 7, message: 2 }, 7, turns)).toEqual(turn(2));
    });

    it("is the whole task for nothing chosen, another session, or a turn no longer kept", () => {
        expect(chosenTurn(null, 7, turns)).toBeNull();
        expect(chosenTurn({ session: 8, message: 2 }, 7, turns)).toBeNull();
        expect(chosenTurn({ session: 7, message: 1 }, 7, turns)).toBeNull();
        expect(chosenTurn({ session: 7, message: 0 }, null, turns)).toBeNull();
    });
});

describe("turnOptions", () => {
    it("offers the whole task first, then the turns, the latest first", () => {
        expect(turnOptions([turn(0), turn(3, { words: "" })], "cell")).toEqual([
            [WHOLE, "Whole cell"],
            ["3", "4. (no words)"],
            ["0", "1. message 0"],
        ]);
        expect(turnOptions([], "task")).toEqual([[WHOLE, "Whole task"]]);
    });

    it("names a turn by its message, counted from 1", () => {
        expect(turnLabel(turn(4, { words: "Fix the loop" }))).toBe("5. Fix the loop");
    });
});

describe("choiceOf", () => {
    it("reads the picker's value as a turn of the session on screen", () => {
        expect(choiceOf("2", 7)).toEqual({ session: 7, message: 2 });
        expect(choiceOf(WHOLE, 7)).toBeNull();
        expect(choiceOf("2", null)).toBeNull();
    });
});

describe("turnNotes", () => {
    it("says nothing of a turn that ended with nobody beside it and nothing left out", () => {
        expect(turnNotes(turn(0), [], true)).toEqual([]);
    });

    it("names who else was at work, a missing end, and what was too large to keep", () => {
        const shared = turn(1, { beside: ["Write the test"], ended: false });
        const notes = turnNotes(shared, [{ path: "dump.csv", bytes: 48_300_000 }], false);
        expect(notes).toEqual([
            "Also at work during this turn: Write the test. What it changed meanwhile is in here too.",
            "This turn has no recorded end yet: these are its changes up to now.",
            "Files over 10 MB that git does not track are not kept, so what this turn did to them is not shown: dump.csv (48.3 MB).",
        ]);
        const two = turnNotes(turn(1, { beside: ["A", "B"] }), [], true);
        expect(two).toEqual([
            "Also at work during this turn: A, B. What they changed meanwhile is in here too.",
        ]);
    });
});

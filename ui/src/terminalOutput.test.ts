import { describe, expect, it } from "vitest";
import type { TerminalOutput } from "./api/types";
import { comesAfter, endedEarly, heard, redrawn } from "./terminalOutput";

const piece = (offset: number, data: string, run = 0): TerminalOutput => ({
    session_id: 1,
    data,
    offset,
    ended: false,
    run,
});
const end = (offset: number, run = 0): TerminalOutput => ({
    ...piece(offset, "", run),
    ended: true,
});

describe("comesAfter", () => {
    it("is for a piece that starts where the replay ends or later, not for one before", () => {
        expect(comesAfter(piece(16, "late"), 16)).toBe(true);
        expect(comesAfter(piece(30, "later"), 16)).toBe(true);
        // In the replay already, though it was only heard after the replay arrived.
        expect(comesAfter(piece(8, "seen"), 16)).toBe(false);
    });
});

describe("redrawn", () => {
    it("is all that was heard early, for a shell that has just started", () => {
        const early = [piece(0, "hello\r\n"), piece(7, "$ ")];
        expect(redrawn({ replay: "", end: 0, run: 0 }, early)).toBe("hello\r\n$ ");
    });

    it("is the replay alone when nothing was heard meanwhile", () => {
        expect(redrawn({ replay: "$ make\r\n", end: 8, run: 0 }, [])).toBe("$ make\r\n");
    });

    it("adds what was printed after the replay was taken, and nothing that is in it", () => {
        const early = [piece(0, "$ make\r\n"), piece(8, "step 1\r\n"), piece(16, "step 2\r\n")];
        const opened = { replay: "$ make\r\nstep 1\r\n", end: 16, run: 0 };
        expect(redrawn(opened, early)).toBe("$ make\r\nstep 1\r\nstep 2\r\n");
    });

    it("goes by where a piece starts, also when the replay lost its beginning", () => {
        const opened = { replay: "end of a long build\r\n", end: 250_000, run: 0 };
        const early = [piece(249_990, "g build\r\n"), piece(250_000, "done\r\n")];
        expect(redrawn(opened, early)).toBe("end of a long build\r\ndone\r\n");
    });

    it("leaves out what a run printed that this one took the place of", () => {
        const early = [piece(0, "old screen", 4), end(10, 4), piece(0, "new screen", 5)];
        expect(redrawn({ replay: "", end: 0, run: 5 }, early)).toBe("new screen");
    });
});

describe("heard", () => {
    const opened = { replay: "$ ", end: 2, run: 5 };

    it("is printed for a piece of the same run that the replay does not hold", () => {
        expect(heard(piece(2, "ls"), { ...opened, run: 0 })).toBe("printed");
        expect(heard(piece(0, "$ ", 5), opened)).toBe("nothing");
    });

    it("is the end only when the run that was opened ends, not one it replaced", () => {
        expect(heard(end(2, 5), opened)).toBe("ended");
        expect(heard(end(40, 4), opened)).toBe("nothing");
        expect(heard(piece(40, "late", 4), opened)).toBe("nothing");
    });
});

describe("endedEarly", () => {
    it("is for a run that ended before its terminal was drawn, and for no other run", () => {
        const opened = { replay: "", end: 0, run: 5 };
        expect(endedEarly(opened, [piece(0, "no such command", 5), end(15, 5)])).toBe(true);
        expect(endedEarly(opened, [end(40, 4), piece(0, "screen", 5)])).toBe(false);
        expect(endedEarly(opened, [])).toBe(false);
    });
});

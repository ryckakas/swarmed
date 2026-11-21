import { describe, expect, it } from "vitest";
import type { TerminalOutput } from "./api/types";
import { comesAfter, redrawn } from "./terminalOutput";

const piece = (offset: number, data: string): TerminalOutput => ({ session_id: 1, data, offset });

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
        expect(redrawn({ replay: "", end: 0 }, early)).toBe("hello\r\n$ ");
    });

    it("is the replay alone when nothing was heard meanwhile", () => {
        expect(redrawn({ replay: "$ make\r\n", end: 8 }, [])).toBe("$ make\r\n");
    });

    it("adds what was printed after the replay was taken, and nothing that is in it", () => {
        const early = [piece(0, "$ make\r\n"), piece(8, "step 1\r\n"), piece(16, "step 2\r\n")];
        const opened = { replay: "$ make\r\nstep 1\r\n", end: 16 };
        expect(redrawn(opened, early)).toBe("$ make\r\nstep 1\r\nstep 2\r\n");
    });

    it("goes by where a piece starts, also when the replay lost its beginning", () => {
        const opened = { replay: "end of a long build\r\n", end: 250_000 };
        const early = [piece(249_990, "g build\r\n"), piece(250_000, "done\r\n")];
        expect(redrawn(opened, early)).toBe("end of a long build\r\ndone\r\n");
    });
});

import { describe, expect, it } from "vitest";
import { outputNote, outputStep } from "./backgroundOutput";

const read = (text: string, start: number, end: number) => ({ text, start, end });

describe("following what a command prints", () => {
    it("shows the first read whole, and says when it is only the latest part", () => {
        expect(outputStep(null, read("hello\n", 0, 6))).toEqual({
            reset: false,
            text: "hello\n",
            end: 6,
            note: null,
        });
        expect(outputStep(null, read("tail\n", 900, 905)).note).toBe("cut");
    });

    it("adds what came since, and starts over when a read does not go on from the last", () => {
        expect(outputStep(6, read("more\n", 6, 11))).toMatchObject({ reset: false, end: 11 });
        expect(outputStep(6, read("latest\n", 4000, 4007))).toMatchObject({ reset: true });
    });

    it("says when the file is gone, and keeps where it was", () => {
        expect(outputStep(6, null)).toEqual({ reset: false, text: "", end: 6, note: "gone" });
        expect(outputNote("gone")).toContain("Claude Code clears it");
    });
});

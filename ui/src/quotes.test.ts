import { describe, expect, it } from "vitest";
import { quotedWords, quoteSpot, withQuote } from "./quotes";

describe("quotedWords", () => {
    it("quotes a selection that lies in one reply, trimmed", () => {
        expect(quotedWords("  the second option \n", "12", "12")).toBe("the second option");
    });

    it("quotes nothing across items, outside a reply, or of nothing but space", () => {
        expect(quotedWords("words", "12", "14")).toBeNull();
        expect(quotedWords("words", null, null)).toBeNull();
        expect(quotedWords("words", "12", null)).toBeNull();
        expect(quotedWords(" \n ", "12", "12")).toBeNull();
    });
});

describe("withQuote", () => {
    const token = '[Quote #1: "words"]';

    it("puts the quote on a line of its own, with the caret on the line under it", () => {
        expect(withQuote("", 0, token)).toEqual({ value: `${token}\n`, caret: token.length + 1 });
        const after = withQuote("About this:", 11, token);
        expect(after.value).toBe(`About this:\n${token}\n`);
        expect(after.caret).toBe(after.value.length);
    });

    it("leaves what is already in the box where it was", () => {
        const between = withQuote("first\nsecond", 6, token);
        expect(between.value).toBe(`first\n${token}\nsecond`);
        expect(between.value.slice(between.caret)).toBe("second");
        const midLine = withQuote("one two", 3, token);
        expect(midLine.value).toBe(`one\n${token}\n two`);
    });
});

describe("quoteSpot", () => {
    const frame = { top: 100, bottom: 600, left: 50, right: 850 };
    const button = { width: 96, height: 28 };
    const line = (top: number, left = 300, right = 500) => ({
        top,
        bottom: top + 20,
        left,
        right,
    });

    it("stands over the start of the selection", () => {
        expect(quoteSpot(line(300), line(340), frame, button)).toEqual({ top: 168, left: 250 });
    });

    it("stands under the end where there is no room above, and inside the frame", () => {
        expect(quoteSpot(line(110), line(150, 70), frame, button)).toEqual({ top: 74, left: 20 });
        expect(quoteSpot(line(300, 840), line(300, 840), frame, button)?.left).toBe(704);
        // A selection taller than the view keeps the button in it.
        expect(quoteSpot(line(50), line(900), frame, button)?.top).toBe(472);
    });

    it("is not there while the selection is out of view", () => {
        expect(quoteSpot(line(20), line(60), frame, button)).toBeNull();
        expect(quoteSpot(line(700), line(740), frame, button)).toBeNull();
    });
});

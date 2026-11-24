import { describe, expect, it } from "vitest";
import {
    expandTokens,
    firstWords,
    forgetTokens,
    isLargePaste,
    messageParts,
    nothingWritten,
    quoteTokenFor,
    tokenFor,
    tokensAgain,
    tokenToRemove,
    withToken,
} from "./pastes";

const report = ["Build ok", "Tests ok", "Lint ok"].join("\n");

describe("a large paste", () => {
    it("is one that runs to many lines or many characters", () => {
        expect(isLargePaste("a\n".repeat(8))).toBe(true);
        expect(isLargePaste("x".repeat(1000))).toBe(true);
        expect(isLargePaste("three\nshort\nlines")).toBe(false);
    });

    it("stands in the box as a numbered token", () => {
        const pastes = nothingWritten();
        expect(tokenFor(report, pastes)).toBe("[Pasted text #1 +3 lines]");
        expect(tokenFor("one line", pastes)).toBe("[Pasted text #2 +1 line]");
        expect(withToken("see  please", 4, 4, "[T]")).toEqual({
            value: "see [T] please",
            caret: 7,
        });
        // What was selected is pasted over.
        expect(withToken("see this please", 4, 8, "[T]").value).toBe("see [T] please");
    });

    it("is taken out whole by one deleting key at its edge", () => {
        const value = "look: [Pasted text #1 +3 lines] and reply";
        const token = { start: 6, end: 31 };
        expect(tokenToRemove("Backspace", value, 31, 31)).toEqual(token);
        expect(tokenToRemove("Delete", value, 6, 6)).toEqual(token);
        // Anywhere else the key does what it always does, as it does with text selected.
        expect(tokenToRemove("Backspace", value, 6, 6)).toBeNull();
        expect(tokenToRemove("Backspace", value, 20, 20)).toBeNull();
        expect(tokenToRemove("Backspace", value, 6, 31)).toBeNull();
        expect(tokenToRemove("a", value, 31, 31)).toBeNull();
    });

    it("is sent in full, and found again in what was sent", () => {
        const pastes = nothingWritten();
        const token = tokenFor(report, pastes);
        const typed = `Read this:\n${token}\nand [Pasted text #9 +2 lines] is just words`;
        const sent = expandTokens(typed, pastes);
        expect(sent).toBe(
            `Read this:\n<pasted_text_1>\n${report}\n</pasted_text_1>\nand [Pasted text #9 +2 lines] is just words`,
        );
        expect(messageParts(sent)).toEqual([
            { kind: "text", text: "Read this:\n" },
            { kind: "paste", number: 1, text: report, lines: 3 },
            { kind: "text", text: "\nand [Pasted text #9 +2 lines] is just words" },
        ]);
        expect(messageParts("no paste here")).toEqual([{ kind: "text", text: "no paste here" }]);
        // A paste that itself holds another paste's closing tag is still kept whole.
        const nested = expandTokens(tokenFor("a\n</pasted_text_1>\nb", pastes), pastes);
        expect(messageParts(nested)).toEqual([
            { kind: "paste", number: 2, text: "a\n</pasted_text_1>\nb", lines: 3 },
        ]);
        forgetTokens(typed, pastes);
        expect([...pastes.paste.keys()]).toEqual([2]);
    });

    it("comes back behind a token when its message is brought back", () => {
        const pastes = nothingWritten();
        const sent = `Read this:\n<pasted_text_4>\n${report}\n</pasted_text_4>\nthanks`;
        const typed = tokensAgain(sent, pastes);
        expect(typed).toBe("Read this:\n[Pasted text #4 +3 lines]\nthanks");
        // Sent again unchanged, it is what it was.
        expect(expandTokens(typed, pastes)).toBe(sent);
        // Its number taken by another paste being written, it is given the next free one.
        expect(tokensAgain(sent, pastes)).toBe("Read this:\n[Pasted text #5 +3 lines]\nthanks");
        expect(tokensAgain("no paste here", pastes)).toBe("no paste here");
    });
});

describe("a quote of a reply", () => {
    const words = 'Option two keeps the cache warm, "for now", and [costs] little.';

    it("stands in the box as a numbered token with its first words", () => {
        const quotes = nothingWritten();
        expect(quoteTokenFor(words, quotes)).toBe(
            '[Quote #1: "Option two keeps the cache warm,…"]',
        );
        expect(quoteTokenFor("Yes.", quotes)).toBe('[Quote #2: "Yes."]');
        // Pastes are numbered apart.
        expect(tokenFor(report, quotes)).toBe("[Pasted text #1 +3 lines]");
    });

    it("shows first words that cannot end its token early, on one line", () => {
        expect(firstWords('say "yes" [now]')).toBe("say 'yes' [now)");
        expect(firstWords("one\n\n  two")).toBe("one two");
        expect(firstWords("x".repeat(40))).toBe(`${"x".repeat(32)}…`);
    });

    it("is taken out whole by one deleting key at its edge", () => {
        const value = 'about [Quote #1: "the first words…"]\nwhy?';
        expect(tokenToRemove("Backspace", value, 36, 36)).toEqual({ start: 6, end: 36 });
        expect(tokenToRemove("Delete", value, 6, 6)).toEqual({ start: 6, end: 36 });
        expect(tokenToRemove("Backspace", value, 30, 30)).toBeNull();
    });

    it("is sent in numbered tags with the comment around it, and found again", () => {
        const quotes = nothingWritten();
        const token = quoteTokenFor(words, quotes);
        const paste = tokenFor(report, quotes);
        const typed = `${token}\nWhy this one?\n${paste}`;
        const sent = expandTokens(typed, quotes);
        expect(sent).toBe(
            `<quoted_reply_1>\n${words}\n</quoted_reply_1>\nWhy this one?\n<pasted_text_1>\n${report}\n</pasted_text_1>`,
        );
        expect(messageParts(sent)).toEqual([
            { kind: "quote", number: 1, text: words },
            // The line break that put the quote on a line of its own is left out.
            { kind: "text", text: "Why this one?\n" },
            { kind: "paste", number: 1, text: report, lines: 3 },
        ]);
        expect(tokensAgain(sent, nothingWritten())).toBe(typed);
        forgetTokens(typed, quotes);
        expect(quotes.quote.size + quotes.paste.size).toBe(0);
    });

    it("is never read for tokens of its own once sent", () => {
        const quotes = nothingWritten();
        const pasted = tokenFor("one", quotes);
        // A reply that names a paste's token is quoted as words, never expanded.
        const sent = expandTokens(quoteTokenFor(`see ${pasted}`, quotes), quotes);
        expect(sent).toBe(`<quoted_reply_1>\nsee ${pasted}\n</quoted_reply_1>`);
    });

    it("reads two quotes in a row, each with its comment", () => {
        const sent =
            "<quoted_reply_1>\nfirst\n</quoted_reply_1>\nyes\n<quoted_reply_2>\nsecond\n</quoted_reply_2>\n\nno";
        expect(messageParts(sent)).toEqual([
            { kind: "quote", number: 1, text: "first" },
            { kind: "text", text: "yes" },
            { kind: "quote", number: 2, text: "second" },
            { kind: "text", text: "\nno" },
        ]);
    });
});

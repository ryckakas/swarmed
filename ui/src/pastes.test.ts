import { describe, expect, it } from "vitest";
import {
    expandPastes,
    forgetPastes,
    isLargePaste,
    messageParts,
    type Pastes,
    tokenFor,
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
        const pastes: Pastes = new Map();
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
        const pastes: Pastes = new Map();
        const token = tokenFor(report, pastes);
        const typed = `Read this:\n${token}\nand [Pasted text #9 +2 lines] is just words`;
        const sent = expandPastes(typed, pastes);
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
        const nested = expandPastes(tokenFor("a\n</pasted_text_1>\nb", pastes), pastes);
        expect(messageParts(nested)).toEqual([
            { kind: "paste", number: 2, text: "a\n</pasted_text_1>\nb", lines: 3 },
        ]);
        forgetPastes(typed, pastes);
        expect([...pastes.keys()]).toEqual([2]);
    });
});

import { describe, expect, it } from "vitest";
import { focusIsFree } from "./focus";

describe("focusIsFree", () => {
    const body = { tagName: "BODY" };

    it("is while nothing has the keyboard", () => {
        expect(focusIsFree(body, body)).toBe(true);
        expect(focusIsFree(null, body)).toBe(true);
    });

    it("is not while a text field, the terminal or a button has it", () => {
        expect(focusIsFree({ tagName: "TEXTAREA" }, body)).toBe(false);
        expect(focusIsFree({ tagName: "INPUT" }, body)).toBe(false);
        expect(focusIsFree({ tagName: "DIV", isContentEditable: true }, body)).toBe(false);
        expect(focusIsFree({ tagName: "BUTTON" }, body)).toBe(false);
    });
});

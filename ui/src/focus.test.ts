import { describe, expect, it } from "vitest";
import { focusIsFree, focusIsIdle } from "./focus";

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

describe("focusIsIdle", () => {
    const body = { tagName: "BODY" };

    it("is while nothing has the keyboard, or an empty text box has it", () => {
        expect(focusIsIdle(body, body)).toBe(true);
        expect(focusIsIdle(null, body)).toBe(true);
        expect(focusIsIdle({ tagName: "TEXTAREA", value: "" }, body)).toBe(true);
    });

    it("is not once something is typed, or while anything else has the keyboard", () => {
        expect(focusIsIdle({ tagName: "TEXTAREA", value: "next" }, body)).toBe(false);
        expect(focusIsIdle({ tagName: "INPUT", value: "" }, body)).toBe(false);
        expect(focusIsIdle({ tagName: "BUTTON" }, body)).toBe(false);
    });
});

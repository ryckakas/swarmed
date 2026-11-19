import { describe, expect, it } from "vitest";
import { type EditPress, editFor } from "./editing";

const press = (key: string, modifiers: Partial<EditPress> = {}): EditPress => ({
    key,
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    ...modifiers,
});

const TEXT = "first line\nsecond line\nthird";

describe("editFor", () => {
    it("moves to the start and the end of the caret's own line", () => {
        // The caret is inside "second".
        expect(editFor(press("Home"), TEXT, 14, 14)).toEqual({
            kind: "select",
            start: 11,
            end: 11,
            direction: "none",
        });
        expect(editFor(press("End"), TEXT, 14, 14)).toEqual({
            kind: "select",
            start: 22,
            end: 22,
            direction: "none",
        });
        expect(editFor(press("End"), TEXT, 25, 25)).toMatchObject({ start: 28, end: 28 });
        expect(editFor(press("Home"), TEXT, 3, 3)).toMatchObject({ start: 0, end: 0 });
    });

    it("selects up to there with Shift", () => {
        expect(editFor(press("Home", { shiftKey: true }), TEXT, 14, 14)).toEqual({
            kind: "select",
            start: 11,
            end: 14,
            direction: "backward",
        });
        expect(editFor(press("End", { shiftKey: true }), TEXT, 14, 14)).toEqual({
            kind: "select",
            start: 14,
            end: 22,
            direction: "forward",
        });
    });

    it("empties the box on ⌘⌫ only with the caret at the very start", () => {
        const erase = press("Backspace", { metaKey: true });
        expect(editFor(erase, TEXT, 0, 0)).toEqual({ kind: "clear" });
        // Elsewhere the system deletes back to the start of the line, as it always does.
        expect(editFor(erase, TEXT, 14, 14)).toBeNull();
        expect(editFor(erase, TEXT, 0, 5)).toBeNull();
        expect(editFor(erase, "", 0, 0)).toBeNull();
    });

    it("leaves every other key to the system", () => {
        expect(editFor(press("a"), TEXT, 0, 0)).toBeNull();
        expect(editFor(press("Home", { ctrlKey: true }), TEXT, 5, 5)).toBeNull();
        expect(editFor(press("Backspace"), TEXT, 0, 0)).toBeNull();
    });
});

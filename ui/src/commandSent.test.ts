import { describe, expect, it } from "vitest";
import { commandJustSent, noteCommandSent } from "./commandSent";

describe("commandJustSent", () => {
    it("is for a moment after a command was sent to that session", () => {
        noteCommandSent(1, 10_000);
        expect(commandJustSent(1, 10_000)).toBe(true);
        expect(commandJustSent(1, 13_000)).toBe(true);
    });

    it("is not later, for another session, or for one never sent a command", () => {
        noteCommandSent(2, 10_000);
        expect(commandJustSent(2, 13_001)).toBe(false);
        expect(commandJustSent(3, 10_000)).toBe(false);
        expect(commandJustSent(2, 9_000)).toBe(false);
    });
});

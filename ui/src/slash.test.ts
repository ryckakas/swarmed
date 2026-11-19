import { describe, expect, it } from "vitest";
import { SUGGESTION_LIMIT, slashMatches, slashQuery, stepActive, withCommand } from "./slash";

describe("slashQuery", () => {
    it("is the name typed so far while a command is being named", () => {
        expect(slashQuery("/")).toBe("");
        expect(slashQuery("/rev")).toBe("rev");
    });

    it("is null once the name is finished, and for any other text", () => {
        expect(slashQuery("/review ")).toBeNull();
        expect(slashQuery("/review the diff")).toBeNull();
        expect(slashQuery("see /review")).toBeNull();
        expect(slashQuery("")).toBeNull();
    });
});

describe("slashMatches", () => {
    const commands = ["security-review", "review", "context", "code-review", "review"];

    it("puts names starting with the query before names containing it", () => {
        expect(slashMatches("rev", commands)).toEqual(["review", "code-review", "security-review"]);
        expect(slashMatches("REV", commands)[0]).toBe("review");
    });

    it("lists everything, sorted and without duplicates, for an empty query", () => {
        expect(slashMatches("", commands)).toEqual([
            "code-review",
            "context",
            "review",
            "security-review",
        ]);
    });

    it("shows a limited number at once", () => {
        const many = Array.from({ length: 30 }, (_, index) => `command-${index}`);
        expect(slashMatches("", many)).toHaveLength(SUGGESTION_LIMIT);
    });
});

describe("choosing", () => {
    it("leaves the name ready for arguments", () => {
        expect(withCommand("review")).toBe("/review ");
    });

    it("wraps round the list in both directions", () => {
        expect(stepActive(2, 1, 3)).toBe(0);
        expect(stepActive(0, -1, 3)).toBe(2);
        expect(stepActive(0, 1, 0)).toBe(0);
    });
});

import { describe, expect, it } from "vitest";
import type { SlashCommand } from "./api/types";
import {
    calledCommand,
    firstPlaceholder,
    kindLabel,
    SUGGESTION_LIMIT,
    sentOn,
    slashMatches,
    slashQuery,
    stepActive,
    takenBySwarmed,
    withCommand,
} from "./slash";

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

describe("firstPlaceholder", () => {
    it("finds the first place to fill in", () => {
        expect(firstPlaceholder("Review {{file}} for {{what}}.")).toEqual([7, 15]);
        expect(firstPlaceholder("{{}}")).toEqual([0, 4]);
    });

    it("finds none in a prompt without one, or with braces that are code", () => {
        expect(firstPlaceholder("Explain this.")).toBeNull();
        expect(firstPlaceholder("const a = { b: {} };")).toBeNull();
        expect(firstPlaceholder("{{open\nclose}}")).toBeNull();
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

describe("what becomes of a command", () => {
    const commands: SlashCommand[] = [
        { name: "usage", kind: "screen", note: "" },
        { name: "model", kind: "setting", note: "" },
        { name: "review", kind: "prompt", note: "" },
        { name: "rename", kind: "terminal", note: "A session is renamed by pressing its title." },
        { name: "vim", kind: "terminal", note: "" },
    ];

    it("is said beside it in the list, unless it is simply sent", () => {
        expect(kindLabel(commands[0])).toBe("shown here");
        expect(kindLabel(commands[1])).toBe("asks here");
        expect(kindLabel(commands[2])).toBeNull();
        expect(kindLabel(commands[3])).toBe(
            "not sent. A session is renamed by pressing its title.",
        );
        expect(kindLabel(commands[4])).toBe("not sent");
        expect(kindLabel(undefined)).toBeNull();
    });

    it("is found by the message's first word", () => {
        expect(calledCommand("/usage", commands)?.name).toBe("usage");
        expect(calledCommand("  /model opus", commands)?.name).toBe("model");
        expect(calledCommand("/usages", commands)).toBeNull();
        expect(calledCommand("see /usage", commands)).toBeNull();
        expect(calledCommand("", commands)).toBeNull();
    });

    it("is handed over at once when Swarmed does it, and waits its turn when the agent does", () => {
        expect(takenBySwarmed("/usage", commands)).toBe(true);
        expect(takenBySwarmed("/model opus", commands)).toBe(true);
        expect(takenBySwarmed("/rename x", commands)).toBe(true);
        expect(takenBySwarmed("/review this", commands)).toBe(false);
        expect(takenBySwarmed("/my-skill", commands)).toBe(false);
        expect(takenBySwarmed("hello", commands)).toBe(false);
    });

    it("leaves a first message only what is sent to the agent", () => {
        expect(sentOn(commands).map((command) => command.name)).toEqual(["review"]);
    });
});

import { describe, expect, it } from "vitest";
import { pickWord, statusKind, wordsFor } from "./wording";

describe("wordsFor", () => {
    it("keeps the plain words in the plain style", () => {
        const words = wordsFor("plain");
        expect(words.newSession).toBe("New session");
        expect(words.overview).toBe("");
        expect(words.newTask).toBe("What should the agent work on?");
        expect(words.status.idle).toEqual(["Idle"]);
        expect(words.status.working).toEqual(["Working"]);
    });

    it("uses the default style's words before the settings are read", () => {
        expect(wordsFor(undefined)).toEqual(wordsFor("swarm"));
    });

    it("names things after the swarm in the Swarm style", () => {
        const words = wordsFor("swarm");
        expect(words.overview).toBe("Hive");
        expect(words.newSession).toBe("Spawn a session");
        expect(words.noSessions).toBe("Nothing spawned yet");
        expect(words.newTask).toBe("What should the swarm work on?");
        expect(words.status.idle).toEqual(["Dormant"]);
        expect(words.status.unread).toEqual(["Hatched"]);
        expect(words.status.working).toEqual(["Mutating", "Incubating"]);
    });

    it("never leaves a status without a word", () => {
        for (const style of ["plain", "swarm"] as const) {
            for (const words of Object.values(wordsFor(style).status)) {
                expect(words.length).toBeGreaterThan(0);
            }
        }
    });
});

describe("statusKind", () => {
    it("counts unread only for an idle session", () => {
        expect(statusKind("idle", true)).toBe("unread");
        expect(statusKind("idle", false)).toBe("idle");
        expect(statusKind("working", true)).toBe("working");
        expect(statusKind("errored", true)).toBe("errored");
    });
});

describe("pickWord", () => {
    it("spreads the rolls evenly over the words", () => {
        const words = ["Mutating", "Incubating"];
        expect(pickWord(words, 0)).toBe("Mutating");
        expect(pickWord(words, 0.49)).toBe("Mutating");
        expect(pickWord(words, 0.5)).toBe("Incubating");
        expect(pickWord(words, 0.999)).toBe("Incubating");
    });

    it("stays within the list whatever the roll", () => {
        expect(pickWord(["Idle"], 0.7)).toBe("Idle");
        expect(pickWord(["a", "b"], 1)).toBe("b");
        expect(pickWord(["a", "b"], -1)).toBe("a");
        expect(pickWord([], 0.3)).toBe("");
    });
});

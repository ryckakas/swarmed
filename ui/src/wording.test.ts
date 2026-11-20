import { describe, expect, it } from "vitest";
import { wordsFor } from "./wording";

describe("wordsFor", () => {
    it("keeps the plain words in every palette but Swarm", () => {
        expect(wordsFor("dusk").newSession).toBe("New session");
        expect(wordsFor("plum").overview).toBe("");
        expect(wordsFor("ember").newTask).toBe("What should the agent work on?");
    });

    it("uses the default palette's words before the settings are read", () => {
        expect(wordsFor(undefined)).toEqual(wordsFor("swarm"));
    });

    it("names things after the swarm in the Swarm palette", () => {
        const words = wordsFor("swarm");
        expect(words.overview).toBe("Hive");
        expect(words.newSession).toBe("Spawn a session");
        expect(words.noSessions).toBe("Nothing spawned yet");
        expect(words.newTask).toBe("What should the swarm work on?");
    });
});

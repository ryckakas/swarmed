import { describe, expect, it } from "vitest";
import { activityWords, pickWord, statusKind, withArticle, wordsFor } from "./wording";

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
        expect(wordsFor(undefined)).toEqual(wordsFor("colony"));
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

    it("counts in the swarm's terms", () => {
        expect(wordsFor("plain").needYou(1)).toBe("1 session needs you");
        expect(wordsFor("plain").needYou(3)).toBe("3 sessions need you");
        expect(wordsFor("plain").unread(2)).toBe("2 unread");
        expect(wordsFor("swarm").needYou(1)).toBe("1 awaits you");
        expect(wordsFor("swarm").needYou(3)).toBe("3 await you");
        expect(wordsFor("swarm").unread(2)).toBe("2 hatched");
        expect(wordsFor("swarm").noTasks).toBe("The nest is empty");
    });

    it("renames the tree only when asked to", () => {
        expect(wordsFor("swarm").addRepository).toBe("Add repository");
        expect(wordsFor("swarm").closed(2)).toBe("2 closed");
        const colony = wordsFor("colony");
        expect(colony.addRepository).toBe("Found a colony");
        expect(colony.startTask).toBe("New brood");
        expect(colony.closed(2)).toBe("2 burrowed");
        expect(colony.reopen).toBe("Unburrow");
        // Everything else is the swarm's.
        expect(colony.status).toEqual(wordsFor("swarm").status);
    });

    it("has other words for what the agent is doing, and leaves the rest alone", () => {
        expect(activityWords(wordsFor("plain"), "Thinking")).toEqual(["Thinking"]);
        expect(activityWords(wordsFor("swarm"), "Thinking")).toContain("Gestating");
        expect(activityWords(wordsFor("swarm"), "Running Bash")).toEqual(["Running Bash"]);
    });

    it("never leaves a status without a word", () => {
        for (const style of ["plain", "swarm", "colony"] as const) {
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

    it("renames closing a session and the environments in the swarm's styles only", () => {
        expect(wordsFor("plain").closeSession).toBe("Close session");
        expect([wordsFor("plain").archiveSession, wordsFor("plain").deleteSession]).toEqual([
            "Archive",
            "Delete",
        ]);
        expect(wordsFor("plain").environments).toBe("Environments");
        for (const style of ["swarm", "colony"] as const) {
            expect(wordsFor(style).closeSession).toBe("Terminate spawn");
            expect(wordsFor(style).closeAsk).toBe("Terminate this spawn?");
            // Put away with its conversation kept, or gone for good.
            expect(wordsFor(style).archiveSession).toBe("Burrow");
            expect(wordsFor(style).files).toBe("Tunnels");
            expect(wordsFor(style).forkAll).toBe("Split this spawn");
            expect(wordsFor(style).deleteSession).toBe("Cull");
            expect(wordsFor(style).environment).toBe("Spawning pool");
        }
    });

    it("puts the right article before a name", () => {
        expect(withArticle("Environment")).toBe("an environment");
        expect(withArticle("Spawning pool")).toBe("a spawning pool");
    });

    it("has the swarm's own words for a turn's end, a stack and the time worked", () => {
        expect(wordsFor("plain").stack.starting).toBe("Stack starting");
        expect(wordsFor("colony").stack).toEqual({
            stopped: "Pool dormant",
            starting: "Pool gestating",
            running: "Pool thriving",
        });
        expect(wordsFor("swarm").turnDone).toBe("Molt complete");
        expect(wordsFor("swarm").timeWorked).toBe("Time mutating");
    });
});

import { describe, expect, it } from "vitest";
import { addedUp, anyInBackground, backgroundShown, NOTHING_IN_BACKGROUND } from "./inBackground";

describe("backgroundShown", () => {
    it("never leaves an idle session with work in the background looking merely idle", () => {
        expect(backgroundShown("idle", { agents: 2, commands: 1 })).toBe("agents");
        expect(backgroundShown("idle", { agents: 0, commands: 3 })).toBe("commands");
        expect(backgroundShown("idle", NOTHING_IN_BACKGROUND)).toBeNull();
    });

    it("leaves it to a status that says more", () => {
        for (const status of ["working", "waiting", "errored"] as const) {
            expect(backgroundShown(status, { agents: 1, commands: 1 })).toBeNull();
        }
    });
});

describe("addedUp", () => {
    it("adds sub-agents and commands apart, and nothing to nothing", () => {
        const counts = [
            { agents: 1, commands: 0 },
            { agents: 2, commands: 3 },
        ];
        expect(addedUp(counts)).toEqual({ agents: 3, commands: 3 });
        expect(addedUp([])).toEqual(NOTHING_IN_BACKGROUND);
        expect(anyInBackground(addedUp([]))).toBe(false);
        expect(anyInBackground({ agents: 0, commands: 1 })).toBe(true);
    });
});

import { describe, expect, it } from "vitest";
import {
    addedUp,
    anyInBackground,
    BROOD_MOST,
    backgroundShown,
    broodBeads,
    broodSize,
    NOTHING_IN_BACKGROUND,
} from "./inBackground";

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

describe("broodSize", () => {
    it("is one grub for each sub-agent at work, up to a few, and none without", () => {
        expect(broodSize({ agents: 0, commands: 4 })).toBe(0);
        expect(broodSize({ agents: 2, commands: 0 })).toBe(2);
        expect(broodSize({ agents: 9, commands: 0 })).toBe(BROOD_MOST);
    });
});

describe("broodBeads", () => {
    it("makes each grub of three beads, head first, each behind smaller and fainter", () => {
        const [head, middle, tail] = broodBeads(1, 2.4);
        expect(broodBeads(1, 2.4)).toHaveLength(3);
        expect(head?.across).toBeLessThan(middle?.across ?? 0);
        expect(middle?.across).toBeLessThan(tail?.across ?? 0);
        expect(head?.r).toBeGreaterThan(middle?.r ?? 0);
        expect(middle?.opacity).toBeGreaterThan(tail?.opacity ?? 0);
    });

    it("has every bead on its way from the first frame, never waiting at the centre", () => {
        for (const bead of broodBeads(3, 2.8)) {
            expect(bead.across).toBeLessThanOrEqual(0);
            expect(bead.upDown).toBeLessThan(bead.across);
        }
    });

    it("keeps the motion up and down a quarter of a turn behind the one across", () => {
        for (const bead of broodBeads(2, 2.4)) {
            expect(bead.across - bead.upDown).toBeCloseTo(0.6);
        }
    });

    it("spreads the grubs evenly round the orbit", () => {
        const heads = broodBeads(3, 2.4).filter((_, at) => at % 3 === 0);
        expect(heads.map((bead) => bead.across)).toEqual([-0.2, -1, -1.8]);
    });

    it("gives no beads for no grubs", () => {
        expect(broodBeads(0, 2.4)).toEqual([]);
    });
});

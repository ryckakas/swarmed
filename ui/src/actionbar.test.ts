import { describe, expect, it } from "vitest";
import { actionLabel, actionTooltip, splitActions } from "./actionbar";
import type { Action } from "./api/types";

const action = (name: string, over: Partial<Action> = {}): Action => ({
    name,
    icon: "",
    prompt: "Do it.",
    command: "",
    of_project: false,
    ...over,
});

describe("splitActions", () => {
    const many = (count: number) => Array.from({ length: count }, (_, at) => action(`a${at}`));

    it("gives every action a button while they fit", () => {
        expect(splitActions(many(3))).toEqual([many(3), []]);
        expect(splitActions([])).toEqual([[], []]);
    });

    it("keeps the rest in a menu, and never a menu of one", () => {
        const [shown, rest] = splitActions(many(12), 6);
        expect(shown).toHaveLength(6);
        expect(rest.map((one) => one.name)).toEqual(["a6", "a7", "a8", "a9", "a10", "a11"]);
        expect(splitActions(many(7), 6)).toEqual([many(7), []]);
    });
});

describe("what an action says", () => {
    it("shows its sign before its name, if it has one", () => {
        expect(actionLabel(action("Tests"))).toBe("Tests");
        expect(actionLabel(action("Tests", { icon: "🧪" }))).toBe("🧪 Tests");
    });

    it("tells in its tooltip what pressing it does", () => {
        expect(actionTooltip(action("Review"))).toBe("Sends the agent: Do it.");
        const runs = action("Env", { prompt: "", command: "nix develop\nmake up" });
        expect(actionTooltip(runs)).toBe("Runs in the terminal: nix develop…");
    });
});

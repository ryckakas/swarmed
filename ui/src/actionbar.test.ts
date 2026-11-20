import { describe, expect, it } from "vitest";
import { ACTION_ICONS, actionLabel, actionTooltip, pickedIcon, splitActions } from "./actionbar";
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

describe("the icons offered for an action", () => {
    it("are all different and short enough to be kept", () => {
        const icons = ACTION_ICONS.map(([icon]) => icon);
        expect(new Set(icons).size).toBe(icons.length);
        // The core keeps an icon of at most 8 characters.
        expect(icons.every((icon) => [...icon].length <= 8)).toBe(true);
    });

    it("are chosen by pressing one, and put back by pressing it again", () => {
        expect(pickedIcon("", "🧪")).toBe("🧪");
        expect(pickedIcon("🔍", "🧪")).toBe("🧪");
        expect(pickedIcon(" 🧪 ", "🧪")).toBe("");
    });
});

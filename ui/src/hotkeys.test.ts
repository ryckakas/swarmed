import { describe, expect, it } from "vitest";
import {
    HOTKEYS,
    keyCaps,
    listedHotkeys,
    matchHotkey,
    stepThrough,
    writtenHotkey,
} from "./hotkeys";

const press = (key: string, held: string[] = []) => ({
    key,
    metaKey: held.includes("cmd"),
    ctrlKey: held.includes("ctrl"),
    shiftKey: held.includes("shift"),
    altKey: held.includes("alt"),
});

describe("matchHotkey", () => {
    it("matches a key with exactly its modifiers", () => {
        expect(matchHotkey(press("n", ["cmd"]))).toBe("new-task");
        expect(matchHotkey(press("W", ["cmd", "shift"]))).toBe("close-session");
        expect(matchHotkey(press("`", ["ctrl"]))).toBe("toggle-terminal");
        expect(matchHotkey(press("Enter", ["cmd"]))).toBe("allow");
        expect(matchHotkey(press("3", ["cmd"]))).toBe("session-3");
        expect(matchHotkey(press("ArrowUp", ["cmd", "alt"]))).toBe("move-up");
        expect(matchHotkey(press("ArrowDown", ["cmd", "alt"]))).toBe("move-down");
        expect(matchHotkey(press("L", ["cmd", "shift"]))).toBe("quote");
        // Plain arrows stay with the sidebar, which moves the focus between its rows.
        expect(matchHotkey(press("ArrowUp"))).toBeNull();
    });

    it("is nothing for plain typing, and leaves the system's own shortcuts alone", () => {
        expect(matchHotkey(press("n"))).toBeNull();
        expect(matchHotkey(press("n", ["cmd", "alt"]))).toBeNull();
        for (const taken of ["w", "q", "m", "h", "c", "v", "x", "a", "z"]) {
            expect(matchHotkey(press(taken, ["cmd"]))).toBeNull();
        }
    });

    it("reads a shifted symbol as the key it sits on", () => {
        expect(matchHotkey(press("+", ["cmd", "shift"]))).toBeNull();
        expect(matchHotkey(press("?", ["cmd"]))).toBe("shortcuts");
    });

    it("has no two shortcuts on the same keys", () => {
        const keys = HOTKEYS.map((hotkey) => hotkey.keys);
        expect(new Set(keys).size).toBe(keys.length);
    });
});

describe("listedHotkeys", () => {
    it("shows the nine numbered shortcuts as one line, where they were", () => {
        const listed = listedHotkeys();
        expect(listed).toHaveLength(HOTKEYS.length - 8);
        const nine = listed.find((hotkey) => hotkey.keys === "cmd+1…9");
        expect(nine?.group).toBe("Sessions");
        expect(keyCaps(nine?.keys ?? "")).toEqual(["⌘", "1…9"]);
    });
});

describe("keyCaps", () => {
    it("writes a shortcut the way macOS does", () => {
        expect(keyCaps("cmd+shift+w")).toEqual(["⌘", "⇧", "W"]);
        expect(keyCaps("ctrl+`")).toEqual(["⌃", "`"]);
        expect(keyCaps("cmd+enter")).toEqual(["⌘", "↩"]);
    });

    it("writes a shortcut of the table in one piece", () => {
        expect(writtenHotkey("move-up")).toBe("⌘⌥↑");
        expect(writtenHotkey("close-session")).toBe("⌘⇧W");
    });
});

describe("stepThrough", () => {
    it("moves one step and wraps round at both ends", () => {
        expect(stepThrough([4, 7, 9], 7, 1)).toBe(9);
        expect(stepThrough([4, 7, 9], 9, 1)).toBe(4);
        expect(stepThrough([4, 7, 9], 4, -1)).toBe(9);
    });

    it("starts at the first when nothing is selected, and has nowhere to go in an empty list", () => {
        expect(stepThrough([4, 7], null, 1)).toBe(4);
        expect(stepThrough([], 4, 1)).toBeNull();
    });
});

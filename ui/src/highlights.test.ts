import { describe, expect, it, vi } from "vitest";
import type { Api } from "./api/client";
import type { Project, Session, Task } from "./api/types";
import {
    choiceName,
    foldsShowing,
    HIGHLIGHT_CHOICES,
    highlightable,
    highlightLabel,
    highlightNow,
    highlightSaid,
    highlightTarget,
    highlightTitle,
    paletteStart,
    paletteStep,
    sameRow,
} from "./highlights";

/** Repository 1, "demo", with task 10, "one", which holds session 7 highlighted blue. */
function sample(task: Partial<Task> = {}): Project[] {
    const session = { id: 7, task_id: 10, highlight: "blue" } as Session;
    const one = {
        id: 10,
        project_id: 1,
        name: "one",
        archived: false,
        done_at: null,
        highlight: "purple",
        sessions: [session],
        ...task,
    } as Task;
    return [{ id: 1, name: "demo", tasks: [one] } as unknown as Project];
}

const TASK = { kind: "task", project: "demo", task: "one" } as const;
const SESSION = { kind: "session", id: 7 } as const;

describe("what can be highlighted", () => {
    it("is a task or a session, never a repository or a cloud task", () => {
        expect(highlightable(TASK)).toBe(true);
        expect(highlightable(SESSION)).toBe(true);
        expect(highlightable({ kind: "project", project: "demo" })).toBe(false);
        expect(highlightable({ kind: "cloud", id: 1 })).toBe(false);
    });

    it("tells one row from another", () => {
        expect(sameRow(SESSION, { kind: "session", id: 7 })).toBe(true);
        expect(sameRow(SESSION, { kind: "session", id: 8 })).toBe(false);
        expect(sameRow(TASK, { ...TASK })).toBe(true);
        expect(sameRow(TASK, { ...TASK, task: "two" })).toBe(false);
        expect(sameRow(SESSION, TASK)).toBe(false);
        expect(sameRow(null, TASK)).toBe(false);
    });

    it("reads a row's colour from the tree, and nothing for a row not in it", () => {
        const projects = sample();
        expect(highlightNow(projects, SESSION)).toBe("blue");
        expect(highlightNow(projects, TASK)).toBe("purple");
        expect(highlightNow(projects, { kind: "session", id: 99 })).toBeUndefined();
        expect(highlightNow(sample({ archived: true }), TASK)).toBeUndefined();
    });
});

describe("the palette", () => {
    it("offers each colour of the marks, then none", () => {
        expect(HIGHLIGHT_CHOICES).toEqual(["green", "blue", "yellow", "red", "purple", null]);
        expect(HIGHLIGHT_CHOICES.map(choiceName)).toEqual([
            "Green",
            "Blue",
            "Yellow",
            "Red",
            "Purple",
            "None",
        ]);
    });

    it("starts on the row's colour, or on the first", () => {
        expect(paletteStart("red")).toBe(3);
        expect(paletteStart(null)).toBe(0);
    });

    it("steps with the arrows, wrapping round, and goes to either end with Home and End", () => {
        expect(paletteStep(0, "ArrowRight")).toBe(1);
        expect(paletteStep(0, "ArrowDown")).toBe(1);
        expect(paletteStep(0, "ArrowLeft")).toBe(5);
        expect(paletteStep(5, "ArrowRight")).toBe(0);
        expect(paletteStep(3, "ArrowUp")).toBe(2);
        expect(paletteStep(3, "Home")).toBe(0);
        expect(paletteStep(1, "End")).toBe(5);
        // Enter and the space bar press the swatch, as on any button.
        expect(paletteStep(2, "Enter")).toBeNull();
        expect(paletteStep(2, " ")).toBeNull();
    });
});

describe("what a highlight says", () => {
    it("names the colour for those who cannot see it", () => {
        expect(highlightSaid("blue")).toBe("highlighted blue");
        expect(highlightSaid(null)).toBe("");
        expect(highlightLabel(null, "Fix the login")).toBe("Highlight Fix the login");
        expect(highlightLabel("red", "Fix the login")).toBe(
            "Fix the login is highlighted red: change or take off the highlight",
        );
    });

    it("names the shortcut that opens the palette too", () => {
        expect(highlightTitle(null)).toContain("⌘⇧H");
        expect(highlightTitle("green")).toMatch(/^Green\. /);
    });
});

describe("opening the palette from the keyboard", () => {
    const none = { folded: [], foldedTasks: [], doneShown: [] };

    it("changes nothing when the row is in view", () => {
        expect(foldsShowing(none, sample(), SESSION)).toBe(none);
        // A task's own row shows with its sessions folded away.
        const tasksFolded = { ...none, foldedTasks: [10] };
        expect(foldsShowing(tasksFolded, sample(), TASK)).toBe(tasksFolded);
    });

    it("unfolds the repository, the task and the Done group that hide the row", () => {
        const folds = { folded: [1, 2], foldedTasks: [10, 11], doneShown: [] };
        expect(foldsShowing(folds, sample(), SESSION)).toEqual({
            folded: [2],
            foldedTasks: [11],
            doneShown: [],
        });
        expect(foldsShowing(none, sample({ done_at: 1 }), TASK)).toEqual({
            ...none,
            doneShown: [1],
        });
        expect(foldsShowing(folds, sample(), { kind: "session", id: 99 })).toBe(folds);
    });
});

describe("asking the core", () => {
    it("highlights a task or a session, or takes it off with null", async () => {
        const api = {
            taskHighlight: vi.fn(async () => undefined),
            sessionHighlight: vi.fn(async () => undefined),
        } as unknown as Api;
        await highlightTarget(api, TASK, "green");
        await highlightTarget(api, SESSION, null);
        expect(api.taskHighlight).toHaveBeenCalledWith("demo", "one", "green");
        expect(api.sessionHighlight).toHaveBeenCalledWith(7, null);
    });
});

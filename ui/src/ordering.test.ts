import { describe, expect, it } from "vitest";
import type { Api } from "./api/client";
import type { Project, Session, Task } from "./api/types";
import {
    dropGap,
    dropMove,
    gripTitle,
    movedAmong,
    orderAttributes,
    orderGroup,
    pinLabel,
    pinnable,
    pinnedAmong,
    pinnedNow,
    pinTarget,
    pinTitle,
    starterHere,
    targetOf,
} from "./ordering";

describe("moving a row among its siblings", () => {
    it("drops before, between or after them", () => {
        // Four siblings; the second (place 1) dragged.
        expect(dropMove(1, 0)).toBe(-1);
        expect(dropMove(1, 1)).toBe(0);
        expect(dropMove(1, 2)).toBe(0);
        expect(dropMove(1, 3)).toBe(1);
        expect(dropMove(1, 4)).toBe(2);
    });

    it("finds the gap by the middles of the rows, and none away from them", () => {
        const rects = [
            { top: 0, bottom: 20 },
            { top: 20, bottom: 40 },
            { top: 40, bottom: 60 },
        ];
        expect(dropGap(5, rects)).toBe(0);
        expect(dropGap(15, rects)).toBe(1);
        expect(dropGap(45, rects)).toBe(2);
        expect(dropGap(55, rects)).toBe(3);
        expect(dropGap(-30, rects)).toBeNull();
        expect(dropGap(100, rects)).toBeNull();
        expect(dropGap(5, [])).toBeNull();
    });

    it("moves one among some, leaving the others in their places", () => {
        const items = ["a", "x", "b", "c"];
        const letters = (item: string) => item !== "x";
        expect(movedAmong(items, letters, (item) => item === "c", -1)).toEqual([
            "a",
            "x",
            "c",
            "b",
        ]);
        expect(movedAmong(items, letters, (item) => item === "a", 9)).toEqual(["b", "x", "c", "a"]);
        expect(movedAmong(items, letters, (item) => item === "x", 1)).toEqual(items);
    });

    it("writes a row's target and siblings onto its element and reads them back", () => {
        const target = { kind: "task", project: "demo", task: "one" } as const;
        const attributes = orderAttributes(target, orderGroup.tasks("demo", false));
        expect(attributes["data-order-group"]).toBe("tasks:demo");
        const element = (written: Record<string, string>) => ({
            getAttribute: (name: string) => written[name] ?? null,
        });
        expect(targetOf(element(attributes))).toEqual(target);
        expect(targetOf(element({}))).toBeNull();
        expect(targetOf(element({ "data-order": "{not json" }))).toBeNull();
    });

    it("names the keys that move a row on the grip it is dragged by", () => {
        expect(gripTitle()).toBe(
            "Drag to move. ⌘⌥↑ and ⌘⌥↓ move the focused row, or the open session.",
        );
    });

    it("groups a started session under its starter only while the starter is listed", () => {
        const session = (id: number, spawned_by: number | null) => ({ id, spawned_by }) as Session;
        const sessions = [session(1, null), session(2, 1), session(3, 9)];
        expect(starterHere(sessions, session(2, 1))).toBe(1);
        expect(starterHere(sessions, session(3, 9))).toBeNull();
        expect(orderGroup.sessions(4, 1, false)).toBe("sessions:4:1");
        expect(orderGroup.sessions(4, null, false)).toBe("sessions:4:top");
    });
});

describe("pinning a row above its siblings", () => {
    const row = (name: string, pinned: boolean) => ({ name, pinned });
    const rows = [row("a", true), row("b", true), row("c", false), row("d", false)];
    const named = (name: string) => (one: { name: string }) => one.name === name;

    it("puts a pinned row last among the pinned, and an unpinned one first among the others", () => {
        expect(pinnedAmong(rows, named("d"), true)).toEqual([
            row("a", true),
            row("b", true),
            row("d", true),
            row("c", false),
        ]);
        expect(pinnedAmong(rows, named("a"), false)).toEqual([
            row("b", true),
            row("a", false),
            row("c", false),
            row("d", false),
        ]);
        expect(pinnedAmong(rows, named("a"), true)).toBe(rows);
        expect(pinnedAmong(rows, named("x"), true)).toBe(rows);
    });

    it("moves pinned rows among the pinned and the others among the others", () => {
        expect(orderGroup.projects(true)).toBe("projects:pinned");
        expect(orderGroup.projects(false)).toBe("projects");
        expect(orderGroup.tasks("demo", true)).toBe("tasks:demo:pinned");
        expect(orderGroup.sessions(4, 1, true)).toBe("sessions:4:1:pinned");
    });

    it("tells whether a row is pinned now, and leaves cloud tasks out", () => {
        const session = { id: 7, pinned: true } as Session;
        const task = { name: "one", archived: false, pinned: false, sessions: [session] } as Task;
        const projects = [{ name: "demo", pinned: true, tasks: [task] } as unknown as Project];
        expect(pinnedNow(projects, { kind: "project", project: "demo" })).toBe(true);
        expect(pinnedNow(projects, { kind: "task", project: "demo", task: "one" })).toBe(false);
        expect(pinnedNow(projects, { kind: "session", id: 7 })).toBe(true);
        expect(pinnedNow(projects, { kind: "session", id: 9 })).toBeNull();
        expect(pinnedNow(projects, { kind: "task", project: "elsewhere", task: "one" })).toBeNull();
        expect(pinnable({ kind: "cloud", id: 1 })).toBe(false);
        expect(pinnable({ kind: "session", id: 1 })).toBe(true);
    });

    it("says what pressing the pin does, with the key that does the same", () => {
        expect(pinTitle(false)).toBe(
            "Pin above the rest. ⌘D pins the focused row, or the open session.",
        );
        expect(pinTitle(true)).toBe(
            "Pinned above the rest. Press to unpin, or ⌘D on the focused row.",
        );
        expect(pinLabel(true, "fix-login")).toBe("Unpin fix-login");
    });

    it("asks the core for the kind of row it is", async () => {
        const asked: string[] = [];
        const api = {
            projectPin: async (project: string, pinned: boolean) => {
                asked.push(`project ${project} ${pinned}`);
            },
            taskPin: async (project: string, task: string, pinned: boolean) => {
                asked.push(`task ${project}/${task} ${pinned}`);
            },
            sessionPin: async (id: number, pinned: boolean) => {
                asked.push(`session ${id} ${pinned}`);
            },
        } as unknown as Api;
        await pinTarget(api, { kind: "project", project: "demo" }, true);
        await pinTarget(api, { kind: "task", project: "demo", task: "one" }, false);
        await pinTarget(api, { kind: "session", id: 3 }, true);
        expect(asked).toEqual(["project demo true", "task demo/one false", "session 3 true"]);
    });
});

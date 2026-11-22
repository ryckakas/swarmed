import { describe, expect, it } from "vitest";
import type { Session } from "./api/types";
import {
    dropGap,
    dropMove,
    movedAmong,
    orderAttributes,
    orderGroup,
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
        const attributes = orderAttributes(target, orderGroup.tasks("demo"));
        expect(attributes["data-order-group"]).toBe("tasks:demo");
        const element = (written: Record<string, string>) => ({
            getAttribute: (name: string) => written[name] ?? null,
        });
        expect(targetOf(element(attributes))).toEqual(target);
        expect(targetOf(element({}))).toBeNull();
        expect(targetOf(element({ "data-order": "{not json" }))).toBeNull();
    });

    it("groups a started session under its starter only while the starter is listed", () => {
        const session = (id: number, spawned_by: number | null) => ({ id, spawned_by }) as Session;
        const sessions = [session(1, null), session(2, 1), session(3, 9)];
        expect(starterHere(sessions, session(2, 1))).toBe(1);
        expect(starterHere(sessions, session(3, 9))).toBeNull();
        expect(orderGroup.sessions(4, 1)).toBe("sessions:4:1");
        expect(orderGroup.sessions(4, null)).toBe("sessions:4:top");
    });
});

import { describe, expect, it } from "vitest";
import type { Descendant, Project, Session, SessionStatus, Task } from "./api/types";
import {
    alsoCloseElsewhereSaid,
    alsoCloseSaid,
    atWorkSaid,
    deepestFirst,
    descendantsElsewhere,
    descendantsOf,
    type KeptSession,
    workerRows,
} from "./closing";
import { wordsFor } from "./wording";

const session = (
    id: number,
    taskId: number,
    parent: number | null,
    status: SessionStatus = "idle",
): Session => ({ id, task_id: taskId, parent, status, title: `s${id}` }) as Session;

const kept = (one: Session, closed = false): KeptSession => ({ session: one, closed });

// An architect (1) in task 10 over planners in 20 (2) and 10 (3); the one in 20 over an
// implementer in 10 (4), and that over a reviewer in 20 (5). 6 works for nobody.
const TREE = [
    kept(session(1, 10, null)),
    kept(session(2, 20, 1)),
    kept(session(3, 10, 1)),
    kept(session(4, 10, 2, "working")),
    kept(session(5, 20, 4, "waiting")),
    kept(session(6, 20, null)),
];

const listed = (descendants: Descendant[]) =>
    descendants.map((one) => [one.session.id, one.depth, one.closed]);

describe("descendantsOf", () => {
    it("lists every session under one, each after the one it works for, the oldest first", () => {
        expect(listed(descendantsOf(TREE, 1, false))).toEqual([
            [2, 1, false],
            [4, 2, false],
            [5, 3, false],
            [3, 1, false],
        ]);
        expect(descendantsOf(TREE, 5, false)).toEqual([]);
    });

    it("walks on below a closed one, which is listed only when asked for", () => {
        const closedPlanner = TREE.map((one) =>
            one.session.id === 2 ? kept(one.session, true) : one,
        );
        expect(listed(descendantsOf(closedPlanner, 1, false))).toEqual([
            [4, 2, false],
            [5, 3, false],
            [3, 1, false],
        ]);
        expect(listed(descendantsOf(closedPlanner, 1, true))[0]).toEqual([2, 1, true]);
    });

    it("ends where a cycle written by hand comes round", () => {
        const round = [kept(session(1, 10, 2)), kept(session(2, 10, 1))];
        expect(listed(descendantsOf(round, 1, false))).toEqual([[2, 1, false]]);
    });
});

describe("descendantsElsewhere", () => {
    it("lists those of other tasks under the task's sessions, each once", () => {
        const ids = descendantsElsewhere(TREE, 10).map((one) => one.session.id);
        expect(ids.sort()).toEqual([2, 5]);
        expect(descendantsElsewhere(TREE, 20).map((one) => one.session.id)).toEqual([4]);
    });
});

describe("deepestFirst", () => {
    it("closes the deepest first and leaves the closed ones out", () => {
        const under = descendantsOf(TREE, 1, false);
        expect(deepestFirst(under)).toEqual([5, 4, 2, 3]);
        const withClosed: Descendant[] = [{ session: session(9, 10, 1), depth: 4, closed: true }];
        expect(deepestFirst(withClosed)).toEqual([]);
    });
});

describe("workerRows", () => {
    const projects = [
        {
            name: "bonsai-lint",
            tasks: [
                {
                    id: 10,
                    name: "rule-engine",
                    sessions: TREE.slice(0, 4)
                        .filter((one) => one.session.task_id === 10)
                        .map((one) => one.session),
                },
                {
                    id: 20,
                    name: "rule-docs",
                    sessions: TREE.filter((one) => one.session.task_id === 20).map(
                        (one) => one.session,
                    ),
                },
            ] as Task[],
        },
    ] as Project[];

    it("names the task of those elsewhere and which are at work, with the live status", () => {
        const fromCore = descendantsOf(TREE, 1, false).map((one) =>
            one.session.id === 3
                ? { ...one, session: { ...one.session, status: "working" as const } }
                : one,
        );
        const rows = workerRows(fromCore, projects, 10);
        expect(rows.map((row) => [row.session.id, row.depth, row.task, row.atWork])).toEqual([
            [2, 1, "rule-docs", false],
            [4, 2, null, true],
            [5, 3, "rule-docs", true],
            // The tree says idle now, whatever the core said when asked.
            [3, 1, null, false],
        ]);
    });

    it("leaves out a closed one, and one the tree no longer holds", () => {
        const gone: Descendant[] = [
            { session: session(2, 20, 1), depth: 1, closed: true },
            { session: session(99, 20, 1), depth: 1, closed: false },
        ];
        expect(workerRows(gone, projects, 10)).toEqual([]);
    });
});

describe("what the dialogs say", () => {
    const plain = wordsFor("plain");

    it("counts the sessions the tick box closes", () => {
        expect(alsoCloseSaid(1, plain)).toBe("Also close the session that works for it");
        expect(alsoCloseSaid(3, plain)).toBe("Also close the 3 sessions that work for it");
        expect(alsoCloseElsewhereSaid(2, plain)).toBe(
            "Also close the 2 sessions in other tasks that work for this task's sessions",
        );
        expect(alsoCloseElsewhereSaid(1, plain)).toBe(
            "Also close the session in another task that works for this task's sessions",
        );
    });

    it("names those at work, whose agents closing stops mid-turn", () => {
        const row = (atWork: boolean) => ({
            session: session(1, 10, null),
            depth: 1,
            task: null,
            atWork,
        });
        expect(atWorkSaid([row(false)], plain)).toBeNull();
        expect(atWorkSaid([row(true), row(false)], plain)).toBe(
            "One of them is at work: closing it stops its agent mid-turn.",
        );
        expect(atWorkSaid([row(true), row(true)], plain)).toBe(
            "2 of them are at work: closing them stops their agents mid-turn.",
        );
    });
});

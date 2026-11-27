import { describe, expect, it } from "vitest";
import type { Project, Session, SessionStatus, Task } from "./api/types";
import { descendantsOf, type KeptSession } from "./closing";
import { wordsFor } from "./wording";
import {
    foldsDown,
    parentLine,
    shownWorkers,
    underSaid,
    WHOLE_UP_TO,
    type WorkerLine,
    workersKey,
    workersOf,
} from "./workers";

const session = (
    id: number,
    taskId: number,
    parent: number | null,
    status: SessionStatus = "idle",
): Session => ({ id, task_id: taskId, parent, status, title: `s${id}` }) as Session;

const kept = (one: Session, closed = false): KeptSession => ({ session: one, closed });

/** The window's tree of `all`: the open ones in their tasks, and how many each has closed. */
function treeOf(all: KeptSession[]): Project[] {
    const task = (id: number, name: string) =>
        ({
            id,
            name,
            archived: false,
            closed: all.filter((one) => one.closed && one.session.task_id === id).length,
            sessions: all
                .filter((one) => !one.closed && one.session.task_id === id)
                .map((one) => one.session),
        }) as Task;
    return [
        {
            name: "bonsai-lint",
            tasks: [task(10, "rule-engine"), task(20, "rule-docs")],
        } as Project,
    ];
}

// An architect (1) in task 10 over a planner in 20 (2) and one in 10 (3); the one in 20 over
// an implementer in 10 (4), and that over a reviewer in 20 (5). 6 works for nobody.
const TREE = [
    kept(session(1, 10, null)),
    kept(session(2, 20, 1)),
    kept(session(3, 10, 1)),
    kept(session(4, 10, 2, "working")),
    kept(session(5, 20, 4, "waiting")),
    kept(session(6, 20, null)),
];

const ARCHITECT = session(1, 10, null);

/** The lines of the architect's header, as the core would list them for `all`. */
const linesOf = (all: KeptSession[], tree = treeOf(all)) =>
    workersOf(descendantsOf(all, 1, true), tree, ARCHITECT);

const drawn = (lines: WorkerLine[]) =>
    lines.map((line) => [
        line.session.id,
        line.depth,
        line.elsewhere?.task.name ?? null,
        line.parent,
        line.under,
    ]);

const closing = (ids: number[]) =>
    TREE.map((one) => (ids.includes(one.session.id) ? kept(one.session, true) : one));

describe("workersOf", () => {
    it("draws everyone under a session at their depth, in whichever task", () => {
        const { open, closed } = linesOf(TREE);
        expect(drawn(open)).toEqual([
            [2, 1, "rule-docs", null, 2],
            [4, 2, null, null, 1],
            [5, 3, "rule-docs", null, 0],
            [3, 1, null, null, 0],
        ]);
        expect(closed).toEqual([]);
        expect(open[0]?.elsewhere?.project).toBe("bonsai-lint");
    });

    it("takes how an open one is doing from the tree, whatever the core said when asked", () => {
        const tree = treeOf(TREE.map((one) => kept({ ...one.session, status: "errored" })));
        const { open } = linesOf(TREE, tree);
        expect(open.map((line) => line.session.status)).toEqual([
            "errored",
            "errored",
            "errored",
            "errored",
        ]);
    });

    it("folds the closed ones apart, unnested, each naming whom it worked for further down", () => {
        const { open, closed } = linesOf(closing([4, 5]));
        expect(drawn(open)).toEqual([
            [2, 1, "rule-docs", null, 0],
            [3, 1, null, null, 0],
        ]);
        expect(drawn(closed)).toEqual([
            [4, 1, null, 2, 0],
            [5, 1, "rule-docs", 4, 0],
        ]);
    });

    it("draws an open one under a closed one beneath the nearest open one, naming the closed", () => {
        // The planner in 20 is done and closed; its implementer and reviewer still work.
        const { open, closed } = linesOf(closing([2]));
        expect(drawn(open)).toEqual([
            [4, 1, null, 2, 1],
            [5, 2, "rule-docs", null, 0],
            [3, 1, null, null, 0],
        ]);
        expect(drawn(closed)).toEqual([[2, 1, "rule-docs", null, 0]]);
    });

    it("leaves out one closed since the core was asked, and counts one reopened as open", () => {
        const listed = descendantsOf(closing([3]), 1, true);
        // Since then, 3 was reopened and 2 closed.
        const { open, closed } = workersOf(listed, treeOf(closing([2])), ARCHITECT);
        expect(drawn(open)).toEqual([
            [4, 1, null, 2, 1],
            [5, 2, "rule-docs", null, 0],
            [3, 1, null, null, 0],
        ]);
        expect(closed).toEqual([]);
    });

    it("follows a session whose parent changed once the core lists it again", () => {
        // The reviewer now works for the architect directly.
        const moved = TREE.map((one) =>
            one.session.id === 5 ? kept({ ...one.session, parent: 1 }) : one,
        );
        expect(drawn(linesOf(moved).open)).toEqual([
            [2, 1, "rule-docs", null, 1],
            [4, 2, null, null, 0],
            [3, 1, null, null, 0],
            [5, 1, "rule-docs", null, 0],
        ]);
        expect(workersKey(treeOf(moved)[0] as Project)).not.toBe(
            workersKey(treeOf(TREE)[0] as Project),
        );
    });

    it("leaves out one whose task is gone, and lists nobody for a session nobody works for", () => {
        const tree = treeOf(TREE);
        const withoutDocs = [{ ...(tree[0] as Project), tasks: [tree[0]?.tasks[0] as Task] }];
        expect(drawn(workersOf(descendantsOf(TREE, 1, true), withoutDocs, ARCHITECT).open)).toEqual(
            [
                [4, 1, null, 2, 0],
                [3, 1, null, null, 0],
            ],
        );
        expect(workersOf([], tree, ARCHITECT)).toEqual({ open: [], closed: [] });
    });
});

describe("parentLine", () => {
    const tree = treeOf(TREE);

    it("draws the session one works for, with its task where that is another", () => {
        const implementer = session(4, 10, 2);
        const line = parentLine(tree, implementer);
        expect(line?.session.id).toBe(2);
        expect(line?.elsewhere?.task.name).toBe("rule-docs");
        expect(parentLine(tree, session(3, 10, 1))?.elsewhere).toBeNull();
    });

    it("draws none for nobody, or for one that is closed", () => {
        expect(parentLine(tree, ARCHITECT)).toBeNull();
        expect(parentLine(treeOf(closing([2])), session(4, 10, 2))).toBeNull();
    });
});

describe("folding a long tree", () => {
    const line = (id: number, depth: number, under = 0): WorkerLine => ({
        session: session(id, 10, null),
        depth,
        elsewhere: null,
        parent: null,
        under,
    });
    // Three planners, of whom the first two have implementers, one with a reviewer under it.
    const LONG = [
        line(1, 1, 4),
        line(2, 2, 1),
        line(3, 3),
        line(4, 2),
        line(5, 2),
        line(6, 1, 3),
        line(7, 2),
        line(8, 2),
        line(9, 2),
        line(10, 1),
    ];
    const ids = (lines: WorkerLine[]) => lines.map((one) => one.session.id);

    it("draws a tree whole up to its limit, and one with nobody further down however long", () => {
        expect(foldsDown(LONG.slice(0, WHOLE_UP_TO))).toBe(false);
        expect(foldsDown(LONG)).toBe(true);
        const direct = Array.from({ length: WHOLE_UP_TO + 4 }, (_, at) => line(at + 1, 1));
        expect(foldsDown(direct)).toBe(false);
        expect(shownWorkers(direct, [])).toBe(direct);
    });

    it("shows those working for it directly, and those under each one unfolded", () => {
        expect(ids(shownWorkers(LONG, []))).toEqual([1, 6, 10]);
        expect(ids(shownWorkers(LONG, [1]))).toEqual([1, 2, 4, 5, 6, 10]);
        expect(ids(shownWorkers(LONG, [1, 2, 6]))).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        // Unfolding one inside a folded one shows nothing until that one is unfolded too.
        expect(ids(shownWorkers(LONG, [2]))).toEqual([1, 6, 10]);
    });

    it("says how many work under a line", () => {
        const plain = wordsFor("plain");
        expect(underSaid(1, plain)).toBe("the session that works under it");
        expect(underSaid(3, plain)).toBe("the 3 sessions that work under it");
    });
});

describe("workersKey", () => {
    const key = (all: KeptSession[]) => workersKey(treeOf(all)[0] as Project);

    it("changes when a session is closed, reopened, started or deleted while closed", () => {
        const now = key(TREE);
        expect(key(closing([3]))).not.toBe(now);
        expect(key([...TREE, kept(session(7, 20, 3))])).not.toBe(now);
        const closedTwice = [...closing([3]), kept(session(8, 10, 1), true)];
        expect(key(closedTwice)).not.toBe(key(closing([3])));
    });

    it("stays the same while only how sessions are doing changes", () => {
        const busy = TREE.map((one) => kept({ ...one.session, status: "working" }));
        expect(key(busy)).toBe(key(TREE));
    });
});

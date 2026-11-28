import { describe, expect, it } from "vitest";
import type { DoneWhy, Project, Pull, Settings, Task } from "./api/types";
import {
    daysWithin,
    doneSaid,
    doneShownFor,
    doneTasks,
    offersPutAway,
    openTasks,
    putAwayDoes,
    putAwayInsteadSaid,
    removedIds,
    removedSaid,
    removeWarning,
} from "./putAway";

const DAY = 86_400;
const NOW = 1_800_000_000;

const task = (
    name: string,
    done: { at: number; why: DoneWhy } | null = null,
    more: Partial<Task> = {},
): Task =>
    ({
        name,
        archived: false,
        done_at: done?.at ?? null,
        done_why: done?.why ?? null,
        pull: null,
        ...more,
    }) as Task;
const project = (tasks: Task[]) => ({ name: "storefront", tasks }) as Project;
const merged = { state: "merged" } as Pull;

describe("openTasks and doneTasks", () => {
    const tasks = [
        task("one"),
        task("old", { at: NOW - 5 * DAY, why: "idle" }),
        task("gone", null, { archived: true }),
        task("late", { at: NOW - DAY, why: "hand" }),
    ];

    it("lists the tasks not put away with the others, and leaves out removed ones", () => {
        expect(openTasks(project(tasks)).map((t) => t.name)).toEqual(["one"]);
    });

    it("lists the done ones apart, the one put away last first", () => {
        expect(doneTasks(project(tasks)).map((t) => t.name)).toEqual(["late", "old"]);
    });
});

describe("offersPutAway", () => {
    it("offers a task whose pull request merged, until it is put away", () => {
        expect(offersPutAway(task("one"), merged)).toBe(true);
        expect(offersPutAway(task("one", { at: NOW, why: "hand" }), merged)).toBe(false);
    });

    it("offers nothing for an open pull request, or none", () => {
        expect(offersPutAway(task("one"), { state: "open" } as Pull)).toBe(false);
        expect(offersPutAway(task("one"), null)).toBe(false);
    });
});

describe("doneSaid", () => {
    it("says when and why a task was put away", () => {
        expect(doneSaid(task("a", { at: NOW - 2 * DAY, why: "merged" }), NOW)).toBe(
            "Put away 2 days ago, after its pull request merged",
        );
        expect(doneSaid(task("a", { at: NOW - DAY, why: "idle" }), NOW)).toBe(
            "Put away yesterday, after it sat idle",
        );
        expect(doneSaid(task("a", { at: NOW - 60, why: "hand" }), NOW)).toBe("Put away today");
    });

    it("says nothing of a task that is not put away", () => {
        expect(doneSaid(task("a"), NOW)).toBeNull();
    });
});

describe("putAwayDoes", () => {
    it("says whether the sessions stay open, as the settings have it", () => {
        expect(putAwayDoes(null)).toContain("keeps its sessions open");
        const closing = { put_away_closes_sessions: true } as Settings;
        expect(putAwayDoes(closing)).toContain("closes its sessions");
    });

    it("says how long the worktree and the branch stay when done tasks are removed", () => {
        const keeping = { put_away_remove_days: 0 } as Settings;
        expect(putAwayDoes(keeping)).toContain("The worktree and the branch stay.");
        const removing = { put_away_remove_days: 30 } as Settings;
        expect(putAwayDoes(removing)).toContain(
            "The worktree and the branch stay for 30 days, then the task is removed.",
        );
        const soon = { put_away_remove_days: 1 } as Settings;
        expect(putAwayDoes(soon)).toContain("stay for 1 day, then");
    });
});

describe("putAwayInsteadSaid", () => {
    it("says it deletes nothing while done tasks stay", () => {
        const keeping = { put_away_remove_days: 0 } as Settings;
        expect(putAwayInsteadSaid(keeping)).toMatch(
            /^Putting it away instead deletes nothing\. Stops its agents/,
        );
        expect(putAwayInsteadSaid(null)).toMatch(/^Putting it away instead deletes nothing\. /);
    });

    it("says it deletes nothing now when done tasks are removed later", () => {
        const removing = { put_away_remove_days: 30 } as Settings;
        const said = putAwayInsteadSaid(removing);
        expect(said).toMatch(/^Putting it away instead deletes nothing now\. /);
        expect(said).toContain("stay for 30 days, then the task is removed.");
    });
});

describe("removedIds", () => {
    const projects = [
        {
            name: "demo",
            tasks: [
                { id: 1, name: "one" },
                { id: 2, name: "two" },
            ],
        },
        { name: "shop", tasks: [{ id: 3, name: "one" }] },
    ] as unknown as Project[];

    it("finds the removed task by its repository and its name", () => {
        expect(removedIds(projects, { project: "demo", task: "one" })).toEqual([1]);
        expect(removedIds(projects, { project: "shop", task: "one" })).toEqual([3]);
    });

    it("finds nothing the tree does not hold", () => {
        expect(removedIds(projects, { project: "demo", task: "three" })).toEqual([]);
        expect(removedIds(projects, { project: "gone", task: "one" })).toEqual([]);
    });
});

describe("removedSaid", () => {
    it("says which task went and how long it had been put away", () => {
        expect(removedSaid({ project: "demo", task: "one", days: 30, notes: [] })).toBe(
            "Removed demo / one, put away for 30 days",
        );
        expect(removedSaid({ project: "demo", task: "one", days: 1, notes: [] })).toBe(
            "Removed demo / one, put away for 1 day",
        );
    });

    it("says what the removal passed over, as Remove… does", () => {
        const notes = ["teardown failed", "kept the branch one: its commits are nowhere else"];
        expect(removedSaid({ project: "demo", task: "one", days: 30, notes })).toBe(
            "Removed demo / one, put away for 30 days, though not everything went as set up: " +
                "teardown failed; kept the branch one: its commits are nowhere else",
        );
    });
});

describe("removeWarning", () => {
    it("warns of lost changes only once done tasks are removed", () => {
        expect(removeWarning(0)).toBeNull();
        expect(removeWarning(30)).toBe(
            "Uncommitted changes in done tasks will be lost when they are removed; those put away longer already go within a minute.",
        );
        expect(removeWarning(1, "broods")).toContain("in done broods");
    });
});

describe("daysWithin", () => {
    it("keeps the days whole and within what the core takes", () => {
        expect(daysWithin("3", 1)).toBe(3);
        expect(daysWithin("2.6", 1)).toBe(3);
        expect(daysWithin("0", 1)).toBe(1);
        expect(daysWithin("0", 0)).toBe(0);
        expect(daysWithin("9000", 1)).toBe(365);
        expect(daysWithin("soon", 1)).toBe(1);
    });
});

describe("doneShownFor", () => {
    const withSession = (done: boolean) =>
        [
            {
                id: 3,
                name: "storefront",
                tasks: [
                    {
                        id: 9,
                        name: "old",
                        archived: false,
                        done_at: done ? NOW : null,
                        sessions: [{ id: 40, task_id: 9 }],
                    },
                ],
            },
        ] as unknown as Project[];

    it("unfolds the Done group of a put away task whose session is opened", () => {
        expect(doneShownFor([], withSession(true), 40)).toEqual([3]);
    });

    it("leaves the list as it was otherwise", () => {
        const shown = [3];
        expect(doneShownFor(shown, withSession(true), 40)).toBe(shown);
        const none: number[] = [];
        expect(doneShownFor(none, withSession(false), 40)).toBe(none);
        expect(doneShownFor(none, withSession(true), 41)).toBe(none);
    });
});

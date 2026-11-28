import { describe, expect, it } from "vitest";
import type { Project, Session, Task } from "./api/types";
import { inbox } from "./inbox";

const NOW = Date.UTC(2026, 10, 28, 12);

const session = (
    id: number,
    status: Session["status"],
    unread = false,
    snoozedUntil = null as number | null,
) => ({ id, status, unread, title: `s${id}`, snoozed_until: snoozedUntil }) as Session;
const task = (name: string, sessions: Session[], archived = false) =>
    ({ name, sessions, archived }) as Task;
const project = (name: string, tasks: Task[]) => ({ name, tasks }) as Project;

describe("inbox", () => {
    const projects = [
        project("storefront", [
            task("one", [session(1, "idle", true), session(2, "working"), session(3, "errored")]),
            task("old", [session(4, "waiting")], true),
        ]),
        project("fsum", [task("two", [session(5, "waiting"), session(6, "idle")])]),
    ];

    it("lists what waits, the most urgent kind first", () => {
        const items = inbox(projects, NOW);
        expect(items.map((item) => [item.session.id, item.kind])).toEqual([
            [5, "waiting"],
            [3, "errored"],
            [1, "unread"],
        ]);
        expect(items[0]).toMatchObject({ project: "fsum", task: "two" });
    });

    it("leaves out busy and idle sessions, and archived tasks", () => {
        const ids = inbox(projects, NOW).map((item) => item.session.id);
        expect(ids).not.toContain(2);
        expect(ids).not.toContain(6);
        expect(ids).not.toContain(4);
    });

    it("counts a busy session's unread flag for nothing", () => {
        expect(inbox([project("p", [task("t", [session(7, "working", true)])])], NOW)).toEqual([]);
    });

    it("leaves out a snoozed session until its time, whatever it waits for", () => {
        const later = NOW / 1000 + 3600;
        const asleep = [
            project("p", [
                task("t", [
                    session(8, "idle", true, later),
                    session(9, "waiting", false, later),
                    session(10, "errored", false, later),
                    session(11, "idle", true, NOW / 1000 - 1),
                ]),
            ]),
        ];
        expect(inbox(asleep, NOW).map((item) => item.session.id)).toEqual([11]);
        // Once its time has come, it is back where it belongs.
        const woken = inbox(asleep, (later + 1) * 1000).map((item) => item.session.id);
        expect(woken).toEqual([9, 10, 8, 11]);
    });

    it("is empty when nothing waits", () => {
        expect(inbox([], NOW)).toEqual([]);
    });
});

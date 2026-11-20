import { describe, expect, it } from "vitest";
import type { Project, Session, Task } from "./api/types";
import { inbox } from "./inbox";

const session = (id: number, status: Session["status"], unread = false) =>
    ({ id, status, unread, title: `s${id}` }) as Session;
const task = (name: string, sessions: Session[], archived = false) =>
    ({ name, sessions, archived }) as Task;
const project = (name: string, tasks: Task[]) => ({ name, tasks }) as Project;

describe("inbox", () => {
    const projects = [
        project("hero", [
            task("one", [session(1, "idle", true), session(2, "working"), session(3, "errored")]),
            task("old", [session(4, "waiting")], true),
        ]),
        project("fsum", [task("two", [session(5, "waiting"), session(6, "idle")])]),
    ];

    it("lists what waits, the most urgent kind first", () => {
        const items = inbox(projects);
        expect(items.map((item) => [item.session.id, item.kind])).toEqual([
            [5, "waiting"],
            [3, "errored"],
            [1, "unread"],
        ]);
        expect(items[0]).toMatchObject({ project: "fsum", task: "two" });
    });

    it("leaves out busy and idle sessions, and archived tasks", () => {
        const ids = inbox(projects).map((item) => item.session.id);
        expect(ids).not.toContain(2);
        expect(ids).not.toContain(6);
        expect(ids).not.toContain(4);
    });

    it("counts a busy session's unread flag for nothing", () => {
        expect(inbox([project("p", [task("t", [session(7, "working", true)])])])).toEqual([]);
    });

    it("is empty when nothing waits", () => {
        expect(inbox([])).toEqual([]);
    });
});

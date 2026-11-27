import { describe, expect, it } from "vitest";
import { sampleProjects } from "./api/mock";
import type { Project, Session, SessionEvent, SessionStatus, Task } from "./api/types";
import { backgroundShown } from "./inBackground";
import { parentChosen, parentOptions, regroupedSaid } from "./parents";
import {
    applySessionEvent,
    countBusy,
    countUnread,
    countWaiting,
    findSession,
    findSessionHome,
    foldedSummary,
    foldedWhenEmptied,
    nestedSessions,
    parentChoices,
    parentElsewhere,
    projectBackground,
    projectStatus,
    projectUnread,
    repositoryOf,
    saysNoSessions,
    stillListed,
    taskBackground,
    taskNameForPrompt,
    taskNameFrom,
    taskStatus,
    titleFrom,
    toggledId,
    unfoldedFor,
    uniqueTaskName,
    withoutIds,
    withoutSession,
    withRead,
    withSession,
    withSessionStatus,
} from "./tree";
import { wordsFor } from "./wording";

/** Task `taskId` of `projects`, which a test expects to be there. */
function findTask(projects: Project[], taskId: number): Task {
    const task = projects.flatMap((project) => project.tasks).find((one) => one.id === taskId);
    if (task === undefined) {
        throw new Error(`no task ${taskId} in the sample`);
    }
    return task;
}

function withTitle(projects: Project[], sessionId: number, title: string): Project[] {
    const session = findSession(projects, sessionId);
    if (session) {
        session.title = title;
    }
    return projects;
}

describe("withSession and withoutSession", () => {
    it("replaces a session with its newer copy", () => {
        const before = sampleProjects();
        const current = findSession(before, 2);
        expect(current).toBeDefined();
        const after = withSession(before, { ...current, id: 2, model: "opus" } as typeof current & {
            id: number;
        });
        expect(findSession(after, 2)?.model).toBe("opus");
        expect(findSession(after, 1)).toBe(findSession(before, 1));
    });

    it("removes a session and nothing else", () => {
        const before = sampleProjects();
        const after = withoutSession(before, 2);
        expect(findSession(after, 2)).toBeUndefined();
        expect(findSession(after, 1)).toEqual(findSession(before, 1));
    });
});

describe("a folded repository", () => {
    it("still says what its most urgent session needs", () => {
        const [storefront, bonsai, fsum] = sampleProjects();
        expect(storefront && projectStatus(storefront)).toBe("waiting");
        expect(bonsai && projectStatus(bonsai)).toBe("errored");
        expect(fsum && projectStatus(fsum)).toBeNull();
    });

    it("folds and unfolds by id", () => {
        expect(toggledId([1], 2)).toEqual([1, 2]);
        expect(toggledId([1, 2], 1)).toEqual([2]);
    });
});

describe("a folded task", () => {
    it("still says how many sessions are open, what the most urgent needs, and if one is unread", () => {
        const [storefront, bonsai] = sampleProjects();
        const nothing = { agents: 0, commands: 0 };
        expect(storefront?.tasks.map(foldedSummary)).toEqual([
            // Each session left a dev server running.
            { count: 2, status: "waiting", unread: false, background: { agents: 0, commands: 2 } },
            // One left a sub-agent at work and a dev server running.
            { count: 2, status: "idle", unread: false, background: { agents: 1, commands: 1 } },
            { count: 0, status: null, unread: false, background: nothing },
            // Put away, its session kept open.
            { count: 1, status: "idle", unread: false, background: nothing },
        ]);
        expect(bonsai?.tasks.map(foldedSummary)).toEqual([
            { count: 2, status: "errored", unread: true, background: nothing },
            // A planner and those working for it, and one more for it in a task of its own.
            { count: 4, status: "idle", unread: false, background: nothing },
            { count: 1, status: "idle", unread: false, background: nothing },
        ]);
    });

    it("unfolds when one of its sessions is opened, and no other task does", () => {
        const projects = sampleProjects();
        const folded = [1, 4];
        expect(unfoldedFor(folded, projects, 5)).toEqual([1]);
        // A session of a task that is not folded, or one the tree does not hold yet.
        expect(unfoldedFor(folded, projects, 3)).toBe(folded);
        expect(unfoldedFor(folded, projects, 99)).toBe(folded);
    });

    it("is forgotten once its task is removed or archived", () => {
        const projects = sampleProjects();
        const folded = [1, 2, 4];
        expect(stillListed(folded, projects)).toBe(folded);
        const archived = projects.map((project) => ({
            ...project,
            tasks: project.tasks.map((task) =>
                task.id === 2 ? { ...task, archived: true } : task,
            ),
        }));
        expect(stillListed([...folded, 9], archived)).toEqual([1, 4]);
    });

    it("says it has no sessions only with none open or closed, and not while set up", () => {
        const shop = findTask(sampleProjects(), 3);
        expect(saysNoSessions(shop)).toBe(true);
        // Its closed line says how it stands: "nothing spawned yet" would be untrue.
        expect(saysNoSessions({ ...shop, closed: 1 })).toBe(false);
        const here = { state: "here", step: "hook", began: 0, latest: null } as const;
        expect(saysNoSessions({ ...shop, setting_up: here })).toBe(false);
        expect(saysNoSessions({ ...shop, setting_up: { state: "elsewhere" } })).toBe(false);
        // Open sessions are listed instead.
        expect(saysNoSessions(findTask(sampleProjects(), 1))).toBe(false);
    });

    it("leaves a list as it was when none of its ids go", () => {
        const ids = [1, 2];
        expect(withoutIds(ids, [3])).toBe(ids);
        expect(withoutIds(ids, [])).toBe(ids);
        expect(withoutIds(ids, [2, 3])).toEqual([1]);
    });
});

/** The tree with task `taskId`'s open sessions gone, and the rest of `change` made to it. */
function emptied(projects: Project[], taskId: number, change: Partial<Task> = {}): Project[] {
    return projects.map((project) => ({
        ...project,
        tasks: project.tasks.map((task) =>
            task.id === taskId ? { ...task, sessions: [], closed: 2, ...change } : task,
        ),
    }));
}

describe("a task left with no open session", () => {
    it("folds as its last open session goes", () => {
        const before = sampleProjects();
        // fix-login-redirect has sessions 1 and 2: the first going leaves it as it is.
        const one = withoutSession(before, 1);
        const folded = [4];
        expect(foldedWhenEmptied(folded, before, one)).toBe(folded);
        expect(foldedWhenEmptied(folded, one, withoutSession(one, 2))).toEqual([4, 1]);
        // Both at once, as a tree read after another Swarmed closed them says it.
        expect(foldedWhenEmptied(folded, before, emptied(before, 1))).toEqual([4, 1]);
        // A chat as well.
        expect(foldedWhenEmptied([], before, withoutSession(before, 6))).toEqual([5]);
    });

    it("folds every task emptied at once, and each only once", () => {
        const before = sampleProjects();
        const after = emptied(emptied(before, 1), 8);
        expect(foldedWhenEmptied([], before, after)).toEqual([1, 8]);
        const folded = [8];
        expect(foldedWhenEmptied(folded, before, emptied(before, 8))).toBe(folded);
    });

    it("stays unfolded once the user unfolds it again", () => {
        const empty = emptied(sampleProjects(), 1);
        const folded: number[] = [];
        expect(foldedWhenEmptied(folded, empty, emptied(empty, 1))).toBe(folded);
    });

    it("is not folded when the tree did not hold it before", () => {
        const folded: number[] = [];
        // The first read after the window opens, and a task that only now appears.
        expect(foldedWhenEmptied(folded, [], sampleProjects())).toBe(folded);
        const before = sampleProjects().map((project) => ({
            ...project,
            tasks: project.tasks.filter((task) => task.id !== 1),
        }));
        expect(foldedWhenEmptied(folded, before, emptied(sampleProjects(), 1))).toBe(folded);
    });

    it("is not folded while it is being set up, whose progress shows under it", () => {
        const before = sampleProjects();
        const folded: number[] = [];
        const here = { state: "here", step: "hook", began: 0, latest: null } as const;
        for (const setting_up of [here, { state: "elsewhere" } as const]) {
            const after = emptied(before, 1, { setting_up });
            expect(foldedWhenEmptied(folded, before, after)).toBe(folded);
        }
        // One whose setup was cut short is set up by nothing: it folds.
        const interrupted = emptied(before, 1, { setting_up: { state: "interrupted" } });
        expect(foldedWhenEmptied(folded, before, interrupted)).toEqual([1]);
    });
});

describe("a renamed session", () => {
    it("takes its new title whatever it was called before", () => {
        const renamed: SessionEvent = {
            session_id: 2,
            status: null,
            event: { type: "renamed", title: "Regression test" },
        };
        const after = applySessionEvent(sampleProjects(), renamed);
        expect(findSession(after, 2)?.title).toBe("Regression test");
    });
});

describe("a marked session", () => {
    const marked = (colour: "green" | null, label: string): SessionEvent => ({
        session_id: 2,
        status: null,
        event: { type: "marked", colour, label },
    });

    it("takes the mark, and loses it when the mark is taken off", () => {
        const after = applySessionEvent(sampleProjects(), marked("green", "e2e passed"));
        expect(findSession(after, 2)?.mark).toEqual({ colour: "green", label: "e2e passed" });
        expect(findSession(applySessionEvent(after, marked(null, "")), 2)?.mark).toBeNull();
    });

    it("loses its mark when the user writes to it", () => {
        const before = applySessionEvent(sampleProjects(), marked("green", ""));
        const written: SessionEvent = {
            session_id: 2,
            status: "working",
            event: { type: "user_message", text: "and now the next thing" },
        };
        expect(findSession(applySessionEvent(before, written), 2)?.mark).toBeNull();
        // What the agent then does leaves a mark alone.
        const done: SessionEvent = { session_id: 2, status: "idle", event: { type: "closed" } };
        expect(findSession(applySessionEvent(before, done), 2)?.mark).not.toBeNull();
    });
});

describe("a session that goes on when a usage limit resets", () => {
    const continuation = (at: number | null): SessionEvent => ({
        session_id: 2,
        status: null,
        event: { type: "continuation", at },
    });

    it("shows when, and no longer once it is cancelled", () => {
        const after = applySessionEvent(sampleProjects(), continuation(1_800_000_000));
        expect(findSession(after, 2)?.continue_at).toBe(1_800_000_000);
        const cancelled = applySessionEvent(after, continuation(null));
        expect(findSession(cancelled, 2)?.continue_at).toBeNull();
    });

    it("no longer waits once a message goes to it", () => {
        const before = applySessionEvent(sampleProjects(), continuation(1_800_000_000));
        const written: SessionEvent = {
            session_id: 2,
            status: "working",
            event: { type: "user_message", text: "carry on", at_reset: true },
        };
        expect(findSession(applySessionEvent(before, written), 2)?.continue_at).toBeNull();
    });
});

describe("withSessionStatus", () => {
    it("replaces one session's status and leaves other branches untouched", () => {
        const before = sampleProjects();
        const after = withSessionStatus(before, 2, "idle");

        expect(findSession(after, 2)?.status).toBe("idle");
        expect(findSession(before, 2)?.status).toBe("working");
        expect(after[1]).toBe(before[1]);
        expect(after[0]?.tasks[1]).toBe(before[0]?.tasks[1]);
    });

    it("changes nothing for an unknown session", () => {
        const before = sampleProjects();
        expect(withSessionStatus(before, 999, "idle")).toEqual(before);
    });
});

describe("applySessionEvent", () => {
    const published = (status: SessionEvent["status"]): SessionEvent => ({
        session_id: 1,
        status,
        event: { type: "text", text: "hello" },
    });

    it("follows the status an event leads to", () => {
        const after = applySessionEvent(sampleProjects(), published("working"));
        expect(findSession(after, 1)?.status).toBe("working");
    });

    it("titles an untitled session from its first message, and only the first", () => {
        const untitled = withTitle(sampleProjects(), 1, "");
        const message = (text: string): SessionEvent => ({
            session_id: 1,
            status: "working",
            event: { type: "user_message", text },
        });
        const once = applySessionEvent(untitled, message("  Fix the login\nwith details"));
        expect(findSession(once, 1)?.title).toBe("Fix the login");
        const twice = applySessionEvent(once, message("And another thing"));
        expect(findSession(twice, 1)?.title).toBe("Fix the login");
    });

    it("keeps the tree as it is when the event changes no status", () => {
        const before = sampleProjects();
        expect(applySessionEvent(before, published(null))).toBe(before);
    });

    it("is unread when the core says a turn handed something back, and only then", () => {
        const done = (unread: boolean | null | undefined): SessionEvent => {
            const event: SessionEvent = {
                session_id: 1,
                status: "idle",
                event: {
                    type: "turn_done",
                    is_error: false,
                    result: null,
                    cost_usd: null,
                    duration_ms: null,
                },
            };
            return unread === undefined ? event : { ...event, unread };
        };
        const read = withRead(sampleProjects(), 1);
        expect(findSession(applySessionEvent(read, done(true)), 1)?.unread).toBe(true);
        // A turn that ends while sub-agents work on in the background hands nothing back yet.
        expect(findSession(applySessionEvent(read, done(null)), 1)?.unread).toBe(false);
        // The sample data's own events say nothing, and any turn's end counts.
        expect(findSession(applySessionEvent(read, done(undefined)), 1)?.unread).toBe(true);
    });
});

describe("titleFrom", () => {
    it("shortens a long first line to the core's limit", () => {
        const title = titleFrom("x".repeat(200));
        expect(title).toHaveLength(60);
        expect(title.endsWith("…")).toBe(true);
    });

    it("reads what a message mentions by a token as its words, as the core does", () => {
        expect(titleFrom("review [#142 Fix the login form] with [@4 Here]\nmore")).toBe(
            "review Fix the login form with Here",
        );
    });
});

describe("repositoryOf", () => {
    it("names a session's repository, and none for a chat", () => {
        const projects = sampleProjects();
        const session = findSession(projects, 3);
        expect(session && repositoryOf(projects, session)).toBe("storefront");
        const chats = projects.find((project) => project.chats);
        const chat = chats?.tasks[0]?.sessions[0];
        expect(chat).toBeDefined();
        expect(chat && repositoryOf(projects, chat)).toBeUndefined();
    });
});

describe("lookups", () => {
    it("finds a session and where it lives", () => {
        const projects = sampleProjects();
        const session = findSession(projects, 3);
        expect(session?.title).toContain("CSV export");
        expect(session?.id).toBe(3);
        const home = session && findSessionHome(projects, session);
        expect(home?.project.name).toBe("storefront");
        expect(home?.task.name).toBe("invoice-export");
        expect(findSession(projects, null)).toBeUndefined();
    });

    it("counts the sessions in a turn: at work or waiting, not idle or failed", () => {
        expect(countBusy(sampleProjects())).toBe(2);
        expect(countBusy([])).toBe(0);
    });

    it("counts the sessions waiting for a decision", () => {
        expect(countWaiting(sampleProjects())).toBe(1);
    });

    it("describes a task by its most urgent session", () => {
        const [storefront, bonsai] = sampleProjects();
        expect(storefront?.tasks.map(taskStatus)).toEqual(["waiting", "idle", null, "idle"]);
        expect(bonsai?.tasks.map(taskStatus)).toEqual(["errored", "idle", "idle"]);
    });
});

describe("taskNameFrom", () => {
    it("turns typed text into a name the core accepts", () => {
        expect(taskNameFrom("Fix the Login redirect!")).toBe("fix-the-login-redirect");
        expect(taskNameFrom("SHOP-142")).toBe("shop-142");
        expect(taskNameFrom("  ../up  ")).toBe("up");
        expect(taskNameFrom("v1..2")).toBe("v1.2");
        expect(taskNameFrom("???")).toBe("");
        expect(taskNameFrom("x".repeat(100))).toHaveLength(64);
    });
});

describe("taskNameForPrompt", () => {
    it("names a task after the first line of its prompt, cut at a word", () => {
        expect(taskNameForPrompt("Fix the login redirect\nUsers get stuck")).toBe(
            "fix-the-login-redirect",
        );
        const long = taskNameForPrompt(
            "Investigate why the nightly export job drops the VAT column",
        );
        expect(long).toBe("investigate-why-the-nightly-export-job");
        expect(taskNameForPrompt("x".repeat(100))).toHaveLength(40);
        expect(taskNameForPrompt("???")).toBe("task");
    });
});

describe("uniqueTaskName", () => {
    it("adds a number only when the name is taken", () => {
        expect(uniqueTaskName("fix", [])).toBe("fix");
        expect(uniqueTaskName("fix", ["fix"])).toBe("fix-2");
        expect(uniqueTaskName("fix", ["fix", "fix-2"])).toBe("fix-3");
    });
});

describe("a change of supervision", () => {
    it("reaches the session in the tree", () => {
        const projects = sampleProjects();
        const changed = applySessionEvent(projects, {
            session_id: 1,
            status: null,
            event: { type: "supervision_changed", supervision: "plan" },
        });
        expect(findSession(changed, 1)?.supervision).toBe("plan");
        expect(findSession(changed, 2)?.supervision).toBe("ask");
    });
});

describe("unread sessions", () => {
    const done: SessionEvent = {
        session_id: 3,
        status: "idle",
        event: { type: "turn_done", is_error: false, result: null, cost_usd: null },
    };

    it("are those whose agent finished a turn, until the user writes or looks", () => {
        const before = sampleProjects();
        expect(findSession(before, 3)?.unread).toBe(false);
        const answered = applySessionEvent(before, done);
        expect(findSession(answered, 3)?.unread).toBe(true);
        expect(countUnread(answered)).toBe(countUnread(before) + 1);

        expect(findSession(withRead(answered, 3), 3)?.unread).toBe(false);
        const written = applySessionEvent(answered, {
            session_id: 3,
            status: "working",
            event: { type: "user_message", text: "and now?" },
        });
        expect(findSession(written, 3)?.unread).toBe(false);
    });

    it("show on a folded repository", () => {
        const [storefront] = applySessionEvent(sampleProjects(), done);
        expect(storefront && projectUnread(storefront)).toBe(true);
    });
});

describe("nestedSessions", () => {
    const [storefront] = sampleProjects();
    const base = storefront?.tasks[0]?.sessions[0] as Session;
    const session = (id: number, parent: number | null): Session => ({
        ...base,
        id,
        parent,
    });
    const shown = (sessions: Session[]) =>
        nestedSessions(sessions).map(({ session: s, depth }) => [s.id, depth]);

    it("lists a session right after the one it works for, one step in", () => {
        const listed = shown([session(1, null), session(2, null), session(3, 1), session(4, 1)]);
        expect(listed).toEqual([
            [1, 0],
            [3, 1],
            [4, 1],
            [2, 0],
        ]);
    });

    it("nests to any depth, each level in the task's own order", () => {
        // 1 over 2 and 5; 2 over 3; 3 over 4.
        const listed = shown([
            session(1, null),
            session(5, 1),
            session(2, 1),
            session(3, 2),
            session(4, 3),
        ]);
        expect(listed).toEqual([
            [1, 0],
            [5, 1],
            [2, 1],
            [3, 2],
            [4, 3],
        ]);
    });

    it("keeps one in line whose parent is not in the task, and says which that is", () => {
        const sessions = [session(2, null), session(5, 1), session(6, 6)];
        expect(shown(sessions)).toEqual([
            [2, 0],
            [5, 0],
            [6, 0],
        ]);
        expect(parentElsewhere(sessions, session(5, 1))).toBe(1);
        expect(parentElsewhere(sessions, session(2, null))).toBeNull();
        expect(parentElsewhere([session(1, null), session(3, 1)], session(3, 1))).toBeNull();
    });

    it("leaves out a cycle written by hand rather than following it", () => {
        expect(shown([session(1, 2), session(2, 1), session(3, null)])).toEqual([[3, 0]]);
    });
});

describe("whom a session can work for", () => {
    const sample = () => sampleProjects();
    const ids = (sessions: Session[]) => sessions.map((session) => session.id);
    const sampled = (projects: ReturnType<typeof sampleProjects>, id: number) => {
        const found = findSession(projects, id);
        if (!found) {
            throw new Error(`no sample session ${id}`);
        }
        return found;
    };

    it("is any other open session of its repository but those that work for it", () => {
        const projects = sample();
        // The rule engine's architect (9) over the parser (10) and the port (12), the parser
        // over its tests (11), and the architect over the docs, in a task of their own (13).
        const parser = sampled(projects, 10);
        expect(ids(parentChoices(projects, parser))).toEqual([9, 12, 4, 5, 13]);
        const planner = sampled(projects, 9);
        expect(ids(parentChoices(projects, planner))).toEqual([4, 5]);
    });

    it("is offered in a menu with nobody first and the current one kept", () => {
        const projects = sample();
        const words = wordsFor("plain");
        const options = parentOptions(projects, sampled(projects, 11), words);
        expect(options[0]).toEqual(["", "Works for nobody"]);
        expect(options).toContainEqual(["10", "Works for session 10: Build the rule parser"]);
        // One that works for a closed session keeps it in the menu, by its number.
        const orphan = { ...sampled(projects, 4), parent: 99 };
        const kept = parentOptions(projects, orphan, words);
        expect(kept[1]).toEqual(["99", "Works for session 99"]);
        expect(parentChosen("")).toBeNull();
        expect(parentChosen("12")).toBe(12);
    });

    it("says in each conversation it concerns what changed", () => {
        const words = wordsFor("plain");
        const change = { child: 11, parent: 9, previous: 10 };
        expect(regroupedSaid(change, 11, words)).toBe(
            "Works for session 9 from here on, no longer for session 10.",
        );
        expect(regroupedSaid(change, 9, words)).toBe(
            "Session 11 works for this session from here on.",
        );
        expect(regroupedSaid(change, 10, words)).toBe(
            "Session 11 no longer works for this session.",
        );
        const freed = { child: 11, parent: null, previous: null };
        expect(regroupedSaid(freed, 11, words)).toBe("Works for nobody from here on.");
    });
});

describe("work left in the background", () => {
    const sample = (id: number): Session => {
        const session = findSession(sampleProjects(), id);
        if (!session) {
            throw new Error(`no sample session ${id}`);
        }
        return session;
    };
    const one = (status: SessionStatus, agents = 0, commands = 0): Session => ({
        ...sample(3),
        status,
        unread: false,
        background: { agents, commands },
    });
    const taskOf = (sessions: Session[]): Task => {
        const task = sampleProjects()[0]?.tasks[1];
        if (!task) {
            throw new Error("no sample task");
        }
        return { ...task, sessions };
    };
    /** What a folded task's heading shows of its sessions: its status, and the ring beside it. */
    const shown = (task: Task) => {
        const status = taskStatus(task);
        return [status, status && backgroundShown(status, taskBackground(task))];
    };

    it("is taken from every event that says it, whatever else the event changes", () => {
        const report: SessionEvent = {
            session_id: 3,
            status: null,
            background: { agents: 1, commands: 2 },
            event: {
                type: "background",
                task_id: "b",
                description: "Review",
                kind: "local_agent",
                state: "running",
                summary: null,
                tool_use_id: null,
                output_file: null,
            },
        };
        const after = applySessionEvent(sampleProjects(), report);
        expect(findSession(after, 3)?.background).toEqual({ agents: 1, commands: 2 });
        expect(findSession(after, 3)?.status).toBe("idle");
        const exited: SessionEvent = {
            session_id: 3,
            status: "errored",
            background: { agents: 0, commands: 0 },
            event: { type: "exited", code: 1, stderr: "" },
        };
        const gone = applySessionEvent(after, exited);
        expect(findSession(gone, 3)?.background).toEqual({ agents: 0, commands: 0 });
        expect(findSession(gone, 3)?.status).toBe("errored");
        // An event that does not say leaves it as it was.
        const said: SessionEvent = {
            session_id: 3,
            status: null,
            event: { type: "text", text: "hi" },
        };
        expect(findSession(applySessionEvent(after, said), 3)?.background).toEqual({
            agents: 1,
            commands: 2,
        });
    });

    it("ranks a folded task above idle: working, then sub-agents, then commands", () => {
        expect(shown(taskOf([one("idle", 1), one("idle", 0, 1)]))).toEqual(["idle", "agents"]);
        expect(shown(taskOf([one("idle"), one("idle", 0, 1)]))).toEqual(["idle", "commands"]);
        expect(shown(taskOf([one("idle"), one("idle")]))).toEqual(["idle", null]);
        expect(shown(taskOf([one("working"), one("idle", 1, 1)]))).toEqual(["working", null]);
        expect(taskBackground(taskOf([one("idle", 1, 2), one("working", 1)]))).toEqual({
            agents: 2,
            commands: 2,
        });
    });

    it("ranks a folded repository the same, over its open tasks only", () => {
        const [storefront] = sampleProjects();
        if (!storefront) {
            throw new Error("no sample repository");
        }
        const withTasks = (tasks: Task[]): Project => ({ ...storefront, tasks });
        const ringOf = (project: Project) => {
            const status = projectStatus(project);
            return status && backgroundShown(status, projectBackground(project));
        };
        const agents = taskOf([one("idle", 1)]);
        const commands = taskOf([one("idle", 0, 1)]);
        expect(ringOf(withTasks([commands, agents]))).toBe("agents");
        expect(ringOf(withTasks([commands, taskOf([one("idle")])]))).toBe("commands");
        expect(ringOf(withTasks([agents, taskOf([one("working")])]))).toBeNull();
        // An archived task's sessions count for nothing.
        expect(
            ringOf(withTasks([{ ...agents, archived: true }, taskOf([one("idle")])])),
        ).toBeNull();
    });

    it("is not a turn: such a session is not counted as busy", () => {
        const projects = sampleProjects();
        const busy = countBusy(projects);
        const idleWithWork = findSession(projects, 8);
        expect(idleWithWork?.status).toBe("idle");
        expect(idleWithWork?.background).toEqual({ agents: 1, commands: 1 });
        expect(busy).toBe(2);
    });
});

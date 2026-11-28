import { describe, expect, it } from "vitest";
import { sampleProjects } from "./api/mock";
import type { Project, Session, Task } from "./api/types";
import { LONGEST_WAIT_MS } from "./rateLimit";
import {
    awakeSessions,
    fieldTime,
    foldedWhenSnoozed,
    nextWake,
    snoozeChoices,
    snoozed,
    snoozedMark,
    snoozedSessions,
    snoozedShownFor,
    statusSeen,
    timeOfField,
    unreadSeen,
    untilWake,
    wakesSnoozed,
    wakeTime,
    wakeTimer,
} from "./snooze";

// Noon on a Saturday, in the time zone the tests run in.
const SATURDAY_NOON = new Date(2026, 10, 28, 12, 0);
const NOW = SATURDAY_NOON.getTime();
const seconds = (ms: number) => Math.floor(ms / 1000);
const HOUR = 3600;
const HOURS = { morning: 9, evening: 18 };

/** A copy of session `id` of `projects`, changed by `change`, wherever it is. */
function withChanged(projects: Project[], id: number, change: Partial<Session>): Project[] {
    const inTask = (task: Task): Task => ({
        ...task,
        sessions: task.sessions.map((one) => (one.id === id ? { ...one, ...change } : one)),
    });
    return projects.map((project) => ({ ...project, tasks: project.tasks.map(inTask) }));
}

/** The sample with session `id` snoozed until `until`, in seconds since 1970. */
const snoozing = (projects: Project[], id: number, until: number | null) =>
    withChanged(projects, id, { snoozed_until: until });

/** Session `id` of the sample, which a test expects to be there. */
function sampleSession(id: number): Session {
    const found = sampleProjects()
        .flatMap((project) => project.tasks)
        .flatMap((task) => task.sessions)
        .find((one) => one.id === id);
    if (!found) {
        throw new Error(`no session ${id} in the sample`);
    }
    return found;
}

describe("a snoozed session", () => {
    const later = seconds(NOW) + HOUR;
    const asleep = { ...sampleSession(5), snoozed_until: later };

    it("is snoozed until its time, and not from then on", () => {
        expect(snoozed(asleep, NOW)).toBe(true);
        expect(snoozed(asleep, later * 1000)).toBe(false);
        expect(snoozed({ snoozed_until: null }, NOW)).toBe(false);
    });

    it("counts as idle and read while snoozed, but working still counts", () => {
        expect(asleep.unread).toBe(true);
        expect(unreadSeen(asleep, NOW)).toBe(false);
        expect(unreadSeen(asleep, later * 1000)).toBe(true);
        expect(statusSeen({ ...asleep, status: "waiting" }, NOW)).toBe("idle");
        expect(statusSeen({ ...asleep, status: "errored" }, NOW)).toBe("idle");
        expect(statusSeen({ ...asleep, status: "working" }, NOW)).toBe("working");
        expect(statusSeen({ ...asleep, status: "waiting" }, later * 1000)).toBe("waiting");
    });

    it("is left out of the task's list and listed in its fold, the first to wake first", () => {
        const one = { ...sampleSession(9), snoozed_until: later + HOUR };
        const two = { ...sampleSession(10), snoozed_until: later };
        const awake = sampleSession(11);
        const sessions = [one, two, awake];
        expect(awakeSessions(sessions, NOW).map((s) => s.id)).toEqual([11]);
        expect(snoozedSessions(sessions, NOW).map((s) => s.id)).toEqual([10, 9]);
    });

    it("shows on its fold's line, the most urgent first, while it is anything but quiet", () => {
        const idle = sampleSession(3);
        const busy = { ...idle, status: "working" as const };
        const failed = { ...idle, status: "errored" as const };
        const asking = { ...idle, status: "waiting" as const };
        const background = sampleSession(8);
        expect(snoozedMark([idle])).toBeNull();
        expect(snoozedMark([idle, busy])?.status).toBe("working");
        expect(snoozedMark([busy, failed])?.status).toBe("errored");
        expect(snoozedMark([failed, asking, busy])?.status).toBe("waiting");
        expect(snoozedMark([idle, background])).toEqual({
            status: "idle",
            background: { agents: 1, commands: 1 },
        });
    });

    it("wakes on what the user writes, a request or a question, and an error alone", () => {
        const typed = { type: "user_message" as const, text: "and now?" };
        expect(wakesSnoozed(typed, "working")).toBe(true);
        expect(wakesSnoozed({ ...typed, remote: true }, "working")).toBe(true);
        // Another session's message and the one sent as a limit resets are not the user's.
        expect(wakesSnoozed({ ...typed, from_session: 9 }, "working")).toBe(false);
        expect(wakesSnoozed({ ...typed, at_reset: true }, "working")).toBe(false);
        const asked = {
            type: "permission_request" as const,
            request_id: "r",
            tool_name: "Bash",
            input: {},
            description: null,
        };
        expect(wakesSnoozed(asked, "waiting")).toBe(true);
        expect(wakesSnoozed({ type: "exited", code: 1, stderr: "" }, "errored")).toBe(true);
        const done = { type: "turn_done" as const, is_error: false, result: null, cost_usd: null };
        expect(wakesSnoozed(done, "idle")).toBe(false);
    });
});

describe("the next wake", () => {
    it("is the soonest time to come among open tasks' sessions", () => {
        const now = seconds(NOW);
        let projects = snoozing(sampleProjects(), 3, now + 2 * HOUR);
        projects = snoozing(projects, 9, now + HOUR);
        // One whose time has passed is awake already.
        projects = snoozing(projects, 10, now - HOUR);
        expect(nextWake(projects, NOW)).toBe(now + HOUR);
        expect(nextWake(projects, (now + HOUR) * 1000)).toBe(now + 2 * HOUR);
        expect(nextWake(sampleProjects(), NOW)).toBeNull();
    });

    it("leaves out a task that is gone from the sidebar", () => {
        const projects = snoozing(sampleProjects(), 3, seconds(NOW) + HOUR).map((project) => ({
            ...project,
            tasks: project.tasks.map((task) => ({ ...task, archived: true })),
        }));
        expect(nextWake(projects, NOW)).toBeNull();
    });

    it("is waited for just past its time, at once if it passed, never longer than a timer can", () => {
        const now = seconds(NOW);
        expect(untilWake(now + 60, NOW)).toBe(60_500);
        expect(untilWake(now - 60, NOW)).toBe(500);
        expect(untilWake(now + 365 * 24 * HOUR, NOW)).toBe(LONGEST_WAIT_MS);
    });

    it("is waited for again and again past the longest timer, until it is reached", () => {
        const wake = seconds(NOW) + 40 * 24 * HOUR;
        const projects = snoozing(sampleProjects(), 3, wake);
        expect(wakeTimer(sampleProjects(), NOW, NOW)).toBeNull();
        // The first timer is cut short at the longest a browser allows.
        const first = wakeTimer(projects, NOW, NOW);
        expect(first).toBe(LONGEST_WAIT_MS);
        // When it runs out the snoozes are looked at again, and the next is set for the rest.
        const looked = NOW + (first ?? 0);
        const second = wakeTimer(projects, looked, looked);
        expect(second).toBe(wake * 1000 - looked + 500);
        // Once the wake is reached and looked at, there is nothing left to wait for.
        const woken = wake * 1000 + 500;
        expect(wakeTimer(projects, woken, woken)).toBeNull();
    });
});

describe("foldedWhenSnoozed", () => {
    const later = seconds(NOW) + HOUR;
    const at = (projects: Project[], now = NOW) => ({ projects, now });

    it("folds a task as its last awake session is snoozed, and no other", () => {
        const before = sampleProjects();
        // rule-docs (8) has session 13 alone; invoice-export (2) has sessions 3 and 8.
        const after = snoozing(snoozing(before, 13, later), 3, later);
        expect(foldedWhenSnoozed([], at(before), at(after))).toEqual([8]);
        expect(foldedWhenSnoozed([1], at(before), at(after))).toEqual([1, 8]);
    });

    it("folds a task whose last awake session is closed, its others snoozed", () => {
        const before = snoozing(sampleProjects(), 3, later);
        const after = before.map((project) => ({
            ...project,
            tasks: project.tasks.map((task) => ({
                ...task,
                sessions: task.sessions.filter((one) => one.id !== 8),
            })),
        }));
        expect(foldedWhenSnoozed([], at(before), at(after))).toEqual([2]);
    });

    it("only as it happens: a task unfolded again stays so, and the first read folds nothing", () => {
        const asleep = snoozing(sampleProjects(), 13, later);
        expect(foldedWhenSnoozed([], at(asleep), at(asleep, NOW + 1000))).toEqual([]);
        expect(foldedWhenSnoozed([], at([]), at(asleep))).toEqual([]);
        const same: number[] = [4];
        expect(foldedWhenSnoozed(same, at(asleep), at(asleep))).toBe(same);
    });

    it("unfolds a task as one of its sessions wakes, on time or early", () => {
        const asleep = snoozing(sampleProjects(), 13, later);
        // On time: nothing in the tree changed, only the time it is looked at.
        expect(foldedWhenSnoozed([8, 1], at(asleep), at(asleep, later * 1000))).toEqual([1]);
        // Early: the snooze was taken off.
        const woken = snoozing(asleep, 13, null);
        expect(foldedWhenSnoozed([1, 8], at(asleep), at(woken))).toEqual([1]);
        // Folded by the user with nothing snoozed, it stays folded while nothing wakes.
        expect(foldedWhenSnoozed([8], at(sampleProjects()), at(woken))).toEqual([8]);
    });

    it("leaves a task being set up unfolded, its progress in view", () => {
        const setting = sampleProjects().map((project) => ({
            ...project,
            tasks: project.tasks.map((task) =>
                task.id === 8 ? { ...task, setting_up: { state: "elsewhere" as const } } : task,
            ),
        }));
        const after = snoozing(setting, 13, later);
        expect(foldedWhenSnoozed([], at(setting), at(after))).toEqual([]);
    });
});

describe("snoozedShownFor", () => {
    it("unfolds the Snoozed fold of a snoozed session's task, and nothing for an awake one", () => {
        const projects = snoozing(sampleProjects(), 13, seconds(NOW) + HOUR);
        const shown: number[] = [];
        expect(snoozedShownFor(shown, projects, 13, NOW)).toEqual([8]);
        expect(snoozedShownFor([8], projects, 13, NOW)).toEqual([8]);
        expect(snoozedShownFor(shown, projects, 3, NOW)).toBe(shown);
        expect(snoozedShownFor(shown, projects, 999, NOW)).toBe(shown);
    });
});

describe("snoozeChoices", () => {
    const said = (now: Date) =>
        snoozeChoices(now, HOURS).map(({ label, at }) => {
            const when = new Date(at * 1000);
            return `${label} ${when.getDate()} ${when.getHours()}:${when.getMinutes()}`;
        });

    it("offers an hour from now, this evening, tomorrow morning and next Monday, soonest first", () => {
        expect(said(SATURDAY_NOON)).toEqual([
            "In 1 hour 28 13:0",
            "This evening 28 18:0",
            "Tomorrow morning 29 9:0",
            "Next Monday 30 9:0",
        ]);
    });

    it("says this morning before its hour, and leaves out this evening once its hour passed", () => {
        expect(said(new Date(2026, 10, 25, 7, 30))).toEqual([
            "In 1 hour 25 8:30",
            "This morning 25 9:0",
            "This evening 25 18:0",
            "Next Monday 30 9:0",
        ]);
        expect(said(new Date(2026, 10, 25, 19, 0))).toEqual([
            "In 1 hour 25 20:0",
            "Tomorrow morning 26 9:0",
            "Next Monday 30 9:0",
        ]);
    });

    it("shows a time once: on a Sunday tomorrow morning is next Monday's", () => {
        expect(said(new Date(2026, 10, 29, 20, 0))).toEqual([
            "In 1 hour 29 21:0",
            "Tomorrow morning 30 9:0",
        ]);
        // On a Monday, next Monday is a week on.
        expect(said(new Date(2026, 10, 30, 12, 0)).at(-1)).toBe("Next Monday 7 9:0");
    });

    it("names a time two choices share by the morning", () => {
        // At 08:00 an hour from now is this morning's hour.
        expect(said(new Date(2026, 10, 25, 8, 0))).toEqual([
            "This morning 25 9:0",
            "This evening 25 18:0",
            "Next Monday 30 9:0",
        ]);
        // With both hours set alike, the evening is the morning.
        const alike = snoozeChoices(new Date(2026, 10, 25, 7, 30), { morning: 9, evening: 9 });
        expect(alike.map((choice) => choice.label)).toEqual([
            "In 1 hour",
            "This morning",
            "Next Monday",
        ]);
    });

    it("takes its hours from the settings, and keeps them across a change of summer time", () => {
        const hours = { morning: 7, evening: 21 };
        // The last Saturday of March: summer time starts in Europe that night.
        const choices = snoozeChoices(new Date(2026, 2, 28, 12, 0), hours);
        const local = choices.map(({ at }) => new Date(at * 1000).getHours());
        expect(local).toEqual([13, 21, 7, 7]);
    });
});

describe("wakeTime", () => {
    it("is the time alone today, with the day within the week, and with the date further off", () => {
        expect(wakeTime(seconds(NOW) + HOUR, SATURDAY_NOON)).toMatch(/^\d/);
        expect(wakeTime(seconds(NOW) + 2 * 24 * HOUR, SATURDAY_NOON)).toMatch(/^\D+ \d/);
        const far = wakeTime(seconds(NOW) + 20 * 24 * HOUR, SATURDAY_NOON);
        expect(far).not.toBe(wakeTime(seconds(NOW) + 6 * 24 * HOUR, SATURDAY_NOON));
        expect(far).toMatch(/\d.*\d/);
    });
});

describe("a time of the user's own", () => {
    it("is read from the field as local time, to the minute, and back", () => {
        const at = timeOfField("2026-11-30T09:15");
        expect(at).toBe(seconds(new Date(2026, 10, 30, 9, 15).getTime()));
        expect(fieldTime(at ?? 0)).toBe("2026-11-30T09:15");
    });

    it("is nothing while the field holds no time", () => {
        expect(timeOfField("")).toBeNull();
        expect(timeOfField("tomorrow")).toBeNull();
        expect(timeOfField("2026-02-30T09:00")).toBeNull();
    });
});

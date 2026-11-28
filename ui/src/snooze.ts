// Snoozing a session: "not now, show me this again at 9:00". Pure, so it can be tested; the
// store, the sidebar, the header, the dialog and the wake timer only act on it.
//
// A snoozed session has a time to wake at (`Session::snoozed_until`, seconds since 1970). Until
// then the sidebar lists it in its task's Snoozed fold instead of with the others, the inbox and
// the Dock's count leave it out, and for its task's and repository's status it counts as idle and
// read. Its agent is never stopped: a snoozed session at work still counts as working, and what
// it has in the background still circles its task, so running work is never hidden.
//
// It wakes early when it needs the user: the core takes the snooze off when the user writes to
// it, on a permission request or a question, when it stops with an error, and when it is closed;
// the window mirrors that from the events (`applySessionEvent` in `tree.ts`). A turn that ends
// well keeps it snoozed. It wakes on time with nothing stored changing: the window keeps the time
// it goes by (`snoozeNow` in the store), and a timer (`useSnoozeWakes`) moves that on when the
// next one is due, which only draws again.
//
// A task folds by itself as its last awake open session is snoozed, and unfolds when one of its
// sessions wakes, only as that happens (`foldedWhenSnoozed`), so that a task the user unfolds
// again stays unfolded.

import type { BackgroundCount, Project, Session, SessionStatus, Task } from "./api/types";
import { addedUp, anyInBackground } from "./inBackground";
import { clockTime, LONGEST_WAIT_MS } from "./rateLimit";
import { isSettingUp } from "./setup";

const HOUR_MS = 3600 * 1000;
const DAY_MS = 24 * HOUR_MS;

/** How long after a wake time what is shown changes, so that the time has surely passed. */
const AFTER_WAKE_MS = 500;

/** Whether `session` is snoozed at `now`, in milliseconds since 1970: its time is still to come. */
export function snoozed(session: Pick<Session, "snoozed_until">, now: number): boolean {
    const until = session.snoozed_until;
    return until !== null && until * 1000 > now;
}

/**
 * The status a session counts as for what asks for the user's attention (the inbox, the Dock's
 * count, a folded task's or repository's mark): its own, unless it is snoozed, when it counts as
 * idle. Working still counts as working: running work is never hidden.
 */
export function statusSeen(session: Session, now: number): SessionStatus {
    if (!snoozed(session, now) || session.status === "working") {
        return session.status;
    }
    return "idle";
}

/** Whether a session counts as holding an answer nobody has looked at: never while snoozed. */
export function unreadSeen(session: Session, now: number): boolean {
    return session.unread && !snoozed(session, now);
}

/** A task's sessions that are awake at `now`: those the sidebar lists, in their order. */
export function awakeSessions(sessions: Session[], now: number): Session[] {
    return sessions.filter((session) => !snoozed(session, now));
}

/** A task's sessions snoozed at `now`, the one to wake first first, as its Snoozed fold lists them. */
export function snoozedSessions(sessions: Session[], now: number): Session[] {
    return sessions
        .filter((session) => snoozed(session, now))
        .sort((one, other) => (one.snoozed_until ?? 0) - (other.snoozed_until ?? 0));
}

/**
 * What a task's Snoozed fold shows of its sessions while one of them works or has something
 * running in the background: the busiest status and what runs, so that work under the fold is
 * never hidden. `null` while none does.
 */
export function snoozedAtWork(
    sessions: Session[],
): { status: SessionStatus; background: BackgroundCount } | null {
    const background = addedUp(sessions.map((session) => session.background));
    if (sessions.some((session) => session.status === "working")) {
        return { status: "working", background };
    }
    return anyInBackground(background) ? { status: "idle", background } : null;
}

/** The open tasks of the tree: those the sidebar lists, put away or not. */
function listedTasks(projects: Project[]): Task[] {
    return projects.flatMap((project) => project.tasks).filter((task) => !task.archived);
}

/**
 * When the next snoozed session of an open task wakes, in seconds since 1970, after `now` in
 * milliseconds; `null` when none is snoozed.
 */
export function nextWake(projects: Project[], now: number): number | null {
    const times = listedTasks(projects)
        .flatMap((task) => task.sessions)
        .filter((session) => snoozed(session, now))
        .map((session) => session.snoozed_until ?? 0);
    return times.length === 0 ? null : Math.min(...times);
}

/**
 * How long to wait from `now`, in milliseconds since 1970, before looking at the snoozes again
 * for one that wakes at `wake`, in seconds since 1970: just past it, at once if it has passed,
 * and never longer than a timer can be set for.
 */
export function untilWake(wake: number, now: number): number {
    return Math.min(Math.max(wake * 1000 - now, 0) + AFTER_WAKE_MS, LONGEST_WAIT_MS);
}

/** The tree at a moment: what the window had, and the time its snoozes were looked at, in ms. */
export interface Seen {
    projects: Project[];
    now: number;
}

/** The ids of the sessions of open tasks snoozed at `seen.now`. */
function snoozedIds(seen: Seen): Set<number> {
    const sessions = listedTasks(seen.projects).flatMap((task) => task.sessions);
    return new Set(sessions.filter((one) => snoozed(one, seen.now)).map((one) => one.id));
}

/** Whether every one of `task`'s open sessions is snoozed at `now`, and it has one. */
function allSnoozed(task: Task, now: number): boolean {
    return task.sessions.length > 0 && task.sessions.every((session) => snoozed(session, now));
}

/**
 * Whether `task`'s last awake session was snoozed between `prior` at `before` and `task` at
 * `after`. Not one being set up, whose progress shows under it.
 */
function fellAsleep(task: Task, prior: Task, before: number, after: number): boolean {
    return allSnoozed(task, after) && !allSnoozed(prior, before) && !isSettingUp(task);
}

/**
 * The folded tasks once the tree went from `before` to `after`: a task whose last awake open
 * session was snoozed (or closed, its others snoozed) folds, since all it would show is its
 * Snoozed fold; a task one of whose sessions woke, on time or early, unfolds to show it. Only as
 * that happens, so a task the user unfolds again stays unfolded; never a task `before` did not
 * hold (the first read, a new task), nor one being set up, whose progress shows under it. The
 * same list if nothing changed.
 */
export function foldedWhenSnoozed(folded: number[], before: Seen, after: Seen): number[] {
    if (before.projects === after.projects && before.now === after.now) {
        return folded;
    }
    const asleep = snoozedIds(before);
    const prior = new Map(listedTasks(before.projects).map((task) => [task.id, task]));
    const woke = new Set<number>();
    const slept: number[] = [];
    for (const task of listedTasks(after.projects)) {
        const then = prior.get(task.id);
        if (then === undefined) {
            continue;
        }
        const awake = task.sessions.filter((session) => !snoozed(session, after.now));
        if (awake.some((session) => asleep.has(session.id))) {
            woke.add(task.id);
        } else if (fellAsleep(task, then, before.now, after.now)) {
            slept.push(task.id);
        }
    }
    const kept = folded.filter((id) => !woke.has(id));
    const added = slept.filter((id) => !kept.includes(id));
    return kept.length === folded.length && added.length === 0 ? folded : [...kept, ...added];
}

/**
 * The tasks whose Snoozed fold is open once session `sessionId` is opened: its task's too, if it
 * is snoozed, so that its row is in view. The same list otherwise.
 */
export function snoozedShownFor(
    shown: number[],
    projects: Project[],
    sessionId: number,
    now: number,
): number[] {
    const session = listedTasks(projects)
        .flatMap((task) => task.sessions)
        .find((one) => one.id === sessionId);
    if (!(session && snoozed(session, now)) || shown.includes(session.task_id)) {
        return shown;
    }
    return [...shown, session.task_id];
}

/** The hours the fixed choices snooze until: the morning's and the evening's, 0 to 23. */
export interface SnoozeHours {
    morning: number;
    evening: number;
}

/** What the settings leave the hours at until the user changes them, as the core does. */
export const DEFAULT_SNOOZE_HOURS: SnoozeHours = { morning: 9, evening: 18 };

/** One of the fixed choices the snooze dialog offers. */
export interface SnoozeChoice {
    label: string;
    /** When it wakes, in seconds since 1970. */
    at: number;
}

/**
 * `now`'s day moved on by `days`, at `hour` o'clock sharp, by the local calendar: set field by
 * field, so that a change to or from summer time in between moves nothing.
 */
function atHour(now: Date, days: number, hour: number): Date {
    const at = new Date(now.getTime());
    at.setDate(at.getDate() + days);
    at.setHours(hour, 0, 0, 0);
    return at;
}

/** How many days from `now` the next Monday is: a week on a Monday, a day on a Sunday. */
function daysToMonday(now: Date): number {
    return (8 - now.getDay()) % 7 || 7;
}

const seconds = (date: Date) => Math.floor(date.getTime() / 1000);

/**
 * The fixed choices, in their one order: in an hour; this evening, left out once its hour has
 * passed; tomorrow morning, or this morning before its hour; next Monday morning. Choices that
 * fall at the same time show once, the first of them.
 */
export function snoozeChoices(now: Date, hours: SnoozeHours): SnoozeChoice[] {
    const evening = atHour(now, 0, hours.evening);
    const morning = atHour(now, 0, hours.morning);
    const all: SnoozeChoice[] = [
        { label: "In 1 hour", at: seconds(new Date(now.getTime() + HOUR_MS)) },
        ...(evening > now ? [{ label: "This evening", at: seconds(evening) }] : []),
        morning > now
            ? { label: "This morning", at: seconds(morning) }
            : { label: "Tomorrow morning", at: seconds(atHour(now, 1, hours.morning)) },
        { label: "Next Monday", at: seconds(atHour(now, daysToMonday(now), hours.morning)) },
    ];
    return all.filter((choice, at) => all.findIndex((one) => one.at === choice.at) === at);
}

/**
 * When a snoozed session wakes, as the window says it: the time alone today, the day's short
 * name before it within the week, and the date as well further off.
 */
export function wakeTime(at: number, now: Date): string {
    if (at * 1000 - now.getTime() < 6 * DAY_MS) {
        return clockTime(at, now);
    }
    const when = new Date(at * 1000);
    const day = when.toLocaleDateString([], { weekday: "short", day: "numeric", month: "short" });
    const time = when.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    return `${day} ${time}`;
}

const two = (n: number) => String(n).padStart(2, "0");

/** A time, in seconds since 1970, as a `datetime-local` field holds it, to the minute: local. */
export function fieldTime(at: number): string {
    const date = new Date(at * 1000);
    const day = `${date.getFullYear()}-${two(date.getMonth() + 1)}-${two(date.getDate())}`;
    return `${day}T${two(date.getHours())}:${two(date.getMinutes())}`;
}

/**
 * The time a `datetime-local` field holds, local, in seconds since 1970; `null` while it holds
 * none, or one that is not a time.
 */
export function timeOfField(value: string): number | null {
    const parts = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/.exec(value);
    if (parts === null) {
        return null;
    }
    const [year = 0, month = 1, day = 1, hour = 0, minute = 0] = parts.slice(1).map(Number);
    const date = new Date(year, month - 1, day, hour, minute);
    const same = date.getMonth() === month - 1 && date.getDate() === day;
    return Number.isNaN(date.getTime()) || !same ? null : seconds(date);
}

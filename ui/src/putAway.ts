// Finished tasks put away (`put_away.rs` in the core): which of a repository's tasks are listed
// with the others and which in its folded Done group, what a done task says of itself, when
// putting one away is offered, and what is said of one removed by itself. Pure, so it can be
// tested.

import type { Project, Pull, Settings, Task } from "./api/types";
import { age } from "./cleanup";
import { findSession, findSessionHome } from "./tree";

/** A repository's tasks listed with the others: neither removed nor put away. */
export function openTasks(project: Project): Task[] {
    return project.tasks.filter((task) => !task.archived && task.done_at === null);
}

/** A repository's tasks that are put away, the one put away last first. */
export function doneTasks(project: Project): Task[] {
    const done = project.tasks.filter((task) => !task.archived && task.done_at !== null);
    return done.sort((a, b) => (b.done_at ?? 0) - (a.done_at ?? 0));
}

/**
 * Whether putting a task away is offered beside it: its pull request, as the core last read it
 * (`pull`), merged, and it is not put away yet. A chat has no pull request, so it is never
 * offered.
 */
export function offersPutAway(task: Task, pull: Pull | null): boolean {
    return task.done_at === null && pull?.state === "merged";
}

/** When and why a done task was put away, as its row and panel say it; `null` while it is not. */
export function doneSaid(task: Task, now: number): string | null {
    if (task.done_at === null) {
        return null;
    }
    const when = age(task.done_at, now);
    switch (task.done_why) {
        case "merged":
            return `Put away ${when}, after its pull request merged`;
        case "idle":
            return `Put away ${when}, after it sat idle`;
        default:
            return `Put away ${when}`;
    }
}

/** A number of days as a sentence says it: "1 day", "30 days". */
function dayCount(days: number): string {
    return days === 1 ? "1 day" : `${days} days`;
}

/**
 * What putting a task away does, by the settings: always its agents and stack, maybe more, and
 * whether it is removed by itself some days later.
 */
export function putAwayDoes(settings: Settings | null): string {
    const sessions = settings?.put_away_closes_sessions
        ? "closes its sessions, keeping their conversations"
        : "keeps its sessions open";
    const removeDays = settings?.put_away_remove_days ?? 0;
    const stay =
        removeDays > 0
            ? `The worktree and the branch stay for ${dayCount(removeDays)}, then the task is removed.`
            : "The worktree and the branch stay.";
    return `Stops its agents and its stack and ${sessions}. ${stay}`;
}

/**
 * What a removal's dialog says of putting the task away instead. With done tasks removed some
 * days later, it deletes nothing now: what goes later is said after it.
 */
export function putAwayInsteadSaid(settings: Settings | null): string {
    const now = (settings?.put_away_remove_days ?? 0) > 0 ? " now" : "";
    return `Putting it away instead deletes nothing${now}. ${putAwayDoes(settings)}`;
}

/** A task the core removed by itself once it had been put away long enough (`task_removed`). */
export interface Removed {
    project: string;
    task: string;
    /** How many days it had been put away, as the setting had it. */
    days: number;
    /** What the removal passed over, as Remove… says it. */
    notes: string[];
}

/** What the window says of a task removed by itself, in the words Remove… uses for its notes. */
export function removedSaid({ project, task, days, notes }: Removed): string {
    const said = `Removed ${project} / ${task}, put away for ${dayCount(days)}`;
    return notes.length === 0
        ? said
        : `${said}, though not everything went as set up: ${notes.join("; ")}`;
}

/**
 * The warning under the setting that removes done tasks, once it removes any: their
 * uncommitted changes go with them, and those put away longer already go within the minute,
 * at the house's next look. `null` while nothing is removed.
 */
export function removeWarning(days: number, tasks = "tasks"): string | null {
    return days > 0
        ? `Uncommitted changes in done ${tasks} will be lost when they are removed; those put away longer already go within a minute.`
        : null;
}

/**
 * The ids of the task the core removed by itself, as the tree before it was read again holds
 * it, so that what the window keeps of it is forgotten. Every task of that name in the
 * repository: one removed before under the same name has nothing left to forget.
 */
export function removedIds(projects: Project[], removed: Pick<Removed, "project" | "task">) {
    const project = projects.find((candidate) => candidate.name === removed.project);
    const tasks = project?.tasks.filter((task) => task.name === removed.task) ?? [];
    return tasks.map((task) => task.id);
}

/** The days a rule waits, as the settings keep them: whole, and within what the core takes. */
export function daysWithin(typed: string, least: number): number {
    const days = Math.round(Number(typed));
    if (!Number.isFinite(days)) {
        return least;
    }
    return Math.min(365, Math.max(least, days));
}

/**
 * The repositories whose Done group is unfolded once session `sessionId` is opened: the one
 * holding its task too, if that was put away, so the session's row is in view. The same list
 * if nothing changes.
 */
export function doneShownFor(shown: number[], projects: Project[], sessionId: number): number[] {
    const session = findSession(projects, sessionId);
    const home = session && findSessionHome(projects, session);
    if (!home || home.task.done_at === null || shown.includes(home.project.id)) {
        return shown;
    }
    return [...shown, home.project.id];
}

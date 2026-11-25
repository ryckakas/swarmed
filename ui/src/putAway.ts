// Finished tasks put away (`put_away.rs` in the core): which of a repository's tasks are listed
// with the others and which in its folded Done group, what a done task says of itself, and when
// putting one away is offered. Pure, so it can be tested.

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

/** What putting a task away does, by the settings: always its agents and stack, maybe more. */
export function putAwayDoes(settings: Settings | null): string {
    const sessions = settings?.put_away_closes_sessions
        ? "closes its sessions, keeping their conversations"
        : "keeps its sessions open";
    return `Stops its agents and its stack and ${sessions}. The worktree and the branch stay.`;
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

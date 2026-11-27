// Closing a session with the sessions that work for it, all the way down, and putting a task
// away with the sessions of other tasks that work for its own (#470): what the dialogs list and
// say, and the walk the sample does as the core does (`parents.rs`). Pure, so it can be tested.

import type { Descendant, Project, Session, SessionStatus } from "./api/types";
import { findSession, findSessionHome } from "./tree";
import type { Words } from "./wording";

/** A session that works for the one asked about, as the dialog lists it. */
export interface WorkerRow {
    session: Session;
    /** 1 for one that works for it, and one more for each step further down. */
    depth: number;
    /** The name of its task, where that is not the task asked about; else `null`. */
    task: string | null;
    /** Whether closing it stops its agent mid-turn. */
    atWork: boolean;
}

/** Whether a session with `status` is in a turn: closing it stops its agent in the middle. */
function inTurn(status: SessionStatus): boolean {
    return status === "working" || status === "waiting";
}

/**
 * The open ones of `descendants`, as the core listed them, in rows for the dialog: each as the
 * tree has it now, so its status is the live one, and with its task named where that is not
 * `taskId`'s. One the tree no longer holds was closed meanwhile, and is left out.
 */
export function workerRows(
    descendants: Descendant[],
    projects: Project[],
    taskId: number,
): WorkerRow[] {
    const rows: WorkerRow[] = [];
    for (const { session: listed, depth, closed } of descendants) {
        const session = closed ? undefined : findSession(projects, listed.id);
        if (!session) {
            continue;
        }
        const home = findSessionHome(projects, session);
        const task = home && home.task.id !== taskId ? home.task.name : null;
        rows.push({ session, depth, task, atWork: inTurn(session.status) });
    }
    return rows;
}

/** The tick box that closes the `count` sessions that work for the one being closed too. */
export function alsoCloseSaid(count: number, words: Words): string {
    const one = words.session;
    return count === 1
        ? `Also close the ${one} that works for it`
        : `Also close the ${count} ${one}s that work for it`;
}

/**
 * The tick box that closes the `count` sessions of other tasks that work for the sessions of
 * the task being put away.
 */
export function alsoCloseElsewhereSaid(count: number, words: Words): string {
    const { task, tasks } = words.terms;
    const one = words.session;
    return count === 1
        ? `Also close the ${one} in another ${task} that works for this ${task}'s ${one}s`
        : `Also close the ${count} ${one}s in other ${tasks} that work for this ${task}'s ${one}s`;
}

/** What the dialog says of those of `rows` at work; `null` for none. */
export function atWorkSaid(rows: WorkerRow[], words: Words): string | null {
    const count = rows.filter((row) => row.atWork).length;
    const { agent, agents } = words.terms;
    if (count === 0) {
        return null;
    }
    return count === 1
        ? `One of them is at work: closing it stops its ${agent} mid-turn.`
        : `${count} of them are at work: closing them stops their ${agents} mid-turn.`;
}

/** A session as the sample keeps it, open or closed. */
export interface KeptSession {
    session: Session;
    closed: boolean;
}

/**
 * Every session under session `id` among `kept`, as the core lists them: each right after the
 * one it works for, the oldest first among those of one. The walk goes on below a closed one,
 * which is listed itself only with `closed`.
 */
export function descendantsOf(kept: KeptSession[], id: number, closed: boolean): Descendant[] {
    const seen = new Set([id]);
    const listed: Descendant[] = [];
    const workersOf = (parent: number, depth: number) =>
        kept
            .filter((one) => one.session.parent === parent && one.session.id !== parent)
            .sort((a, b) => a.session.id - b.session.id)
            .filter((one) => !seen.has(one.session.id) && seen.add(one.session.id))
            .map((one) => ({ ...one, depth: depth + 1 }))
            .reverse();
    const stack = workersOf(id, 0);
    for (let next = stack.pop(); next; next = stack.pop()) {
        listed.push({ session: next.session, depth: next.depth, closed: next.closed });
        stack.push(...workersOf(next.session.id, next.depth));
    }
    return listed.filter((one) => closed || !one.closed);
}

/**
 * The open sessions of other tasks that work for one of task `taskId`'s, its closed ones
 * included, however far down: each once, as the core lists them.
 */
export function descendantsElsewhere(kept: KeptSession[], taskId: number): Descendant[] {
    const own = kept.filter((one) => one.session.task_id === taskId);
    const seen = new Set<number>();
    return own
        .flatMap((one) => descendantsOf(kept, one.session.id, false))
        .filter((one) => one.session.task_id !== taskId)
        .filter((one) => !seen.has(one.session.id) && seen.add(one.session.id));
}

/** The open ones of `descendants` in the order the core closes them: the deepest first. */
export function deepestFirst(descendants: Descendant[]): number[] {
    const open = descendants.filter((one) => !one.closed);
    return [...open].sort((a, b) => b.depth - a.depth).map((one) => one.session.id);
}

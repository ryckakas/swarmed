// Everyone who works for a session, all the way down and in whichever task (#503): the tree its
// header draws, with the closed ones folded below the open ones. Which sessions are listed, and
// under whom, is the core's (`session_descendants`); how an open one is doing is the tree's,
// which hears every event. Pure, so it can be tested.

import type { BackgroundCount, Descendant, Project, Session, Task } from "./api/types";
import { addedUp } from "./inBackground";
import { findSession, findSessionHome } from "./tree";
import { type StatusKind, statusKind, type Words } from "./wording";

/**
 * Up to this many open sessions, the tree is drawn whole; with more, and some of them further
 * down, only those that work for the session directly, each with how many work under it.
 */
export const WHOLE_UP_TO = 8;

/** A session that works with the one on screen, as its header draws it. */
export interface WorkerLine {
    session: Session;
    /** How far in it is drawn: 1 under the session on screen, one more for each open one between. */
    depth: number;
    /** Its repository and task, where that is not the task of the session on screen; else `null`. */
    elsewhere: { project: string; task: Task } | null;
    /** The session it works for, where the tree does not draw that one right above it; else `null`. */
    parent: number | null;
    /** How many open ones are drawn under it, however far down. */
    under: number;
}

/** Those that work for a session, as its header lists them. */
export interface Workers {
    /** The open ones, each right after the one it works for, nested as they work for one another. */
    open: WorkerLine[];
    /** The closed ones, in the same order, not nested: one quiet list folded below. */
    closed: WorkerLine[];
}

const NOBODY: Workers = { open: [], closed: [] };

/** One step of the way down from the session on screen: a session, and whether it is drawn open. */
interface Step {
    id: number;
    drawn: boolean;
}

/**
 * The lines of those working for `own`, from what the core `listed` with the closed ones (each
 * right after the one it works for, with its depth). An open one is drawn as the tree has it now,
 * under the nearest open one above it, and names the closed one it works for where that is not
 * drawn above it. One the core listed as open that the tree no longer holds was closed meanwhile,
 * and waits for the list read again; so does one whose task is gone. One listed as closed that
 * the tree holds was reopened meanwhile, and is open.
 */
export function workersOf(listed: Descendant[], projects: Project[], own: Session): Workers {
    if (listed.length === 0) {
        return NOBODY;
    }
    const open: WorkerLine[] = [];
    const closed: WorkerLine[] = [];
    const path: Step[] = [];
    for (const one of listed) {
        path.length = Math.max(one.depth - 1, 0);
        const placed = placedBelow(path, one, projects, own);
        if (placed) {
            (placed.open ? open : closed).push(placed.line);
        }
    }
    return { open: withUnder(open), closed };
}

/**
 * Where `one` goes, below the steps of `path`, to which it is added: drawn open, folded closed,
 * or nowhere (`null`).
 */
function placedBelow(
    path: Step[],
    one: Descendant,
    projects: Project[],
    own: Session,
): { line: WorkerLine; open: boolean } | null {
    const above = path.at(-1);
    const live = findSession(projects, one.session.id);
    const line = lineOf(projects, live ?? (one.closed ? one.session : undefined), own);
    const drawn = live !== undefined && line !== null;
    path.push({ id: one.session.id, drawn });
    if (!line) {
        return null;
    }
    if (!drawn) {
        return { line: { ...line, parent: above?.id ?? null }, open: false };
    }
    const depth = path.filter((step) => step.drawn).length;
    const parent = above?.drawn === false ? above.id : null;
    return { line: { ...line, depth, parent }, open: true };
}

/** `lines` with how many are drawn under each: those after it, up to the next one as far out. */
function withUnder(lines: WorkerLine[]): WorkerLine[] {
    return lines.map((line, at) => {
        const after = lines.slice(at + 1);
        const end = after.findIndex((next) => next.depth <= line.depth);
        return { ...line, under: end === -1 ? after.length : end };
    });
}

/**
 * `session` as a line of its own, with its task where that is not `own`'s; `null` for none, or
 * for one whose task the tree no longer holds.
 */
function lineOf(
    projects: Project[],
    session: Session | undefined,
    own: Session,
): WorkerLine | null {
    const home = session && findSessionHome(projects, session);
    if (!(session && home)) {
        return null;
    }
    const elsewhere =
        home.task.id === own.task_id ? null : { project: home.project.name, task: home.task };
    return { session, depth: 1, elsewhere, parent: null, under: 0 };
}

/** The line of the open session `own` works for, as the header draws it; `null` for none. */
export function parentLine(projects: Project[], own: Session): WorkerLine | null {
    return lineOf(projects, findSession(projects, own.parent), own);
}

/** Whether the tree is too long to draw whole: it then folds to those working for it directly. */
export function foldsDown(open: WorkerLine[]): boolean {
    return open.length > WHOLE_UP_TO && open.some((line) => line.depth > 1);
}

/**
 * The open lines shown: all of them while the tree is drawn whole; else those working for the
 * session directly, and under each of `unfolded` those working for it in turn.
 */
export function shownWorkers(open: WorkerLine[], unfolded: readonly number[]): WorkerLine[] {
    if (!foldsDown(open)) {
        return open;
    }
    const shown: WorkerLine[] = [];
    let hiddenBelow = Number.POSITIVE_INFINITY;
    for (const line of open) {
        if (line.depth > hiddenBelow) {
            continue;
        }
        const folded = line.under > 0 && !unfolded.includes(line.session.id);
        hiddenBelow = folded ? line.depth : Number.POSITIVE_INFINITY;
        shown.push(line);
    }
    return shown;
}

/** How many of those working for a session are doing one thing, as the folded line counts them. */
export interface StatusCount {
    kind: StatusKind;
    count: number;
    /** What they have running in the background, added up, which rings their mark. */
    background: BackgroundCount;
    /** The count in words: "2 working". */
    said: string;
}

/** The one line a session's header folds those working for it into. */
export interface WorkersSummary {
    /** What leads the line: "9 working for it", or only "Working for it" with none open. */
    lead: string;
    /** The open ones counted by how they are doing, the most urgent first; none with no count. */
    counts: StatusCount[];
    /** The closed ones in words, "2 closed"; `null` for none. */
    closed: string | null;
    /** All of it in words, for the button that unfolds it. */
    said: string;
}

/** How sessions can be doing, the most urgent first, as the sidebar ranks them. */
const URGENCY: StatusKind[] = ["waiting", "errored", "working", "unread", "idle"];

/**
 * Those working for a session on one line: how many are open, counted by how they are doing with
 * the most urgent first, and how many are closed. "9 working for it: 1 needs you, 2 working,
 * 6 idle, 2 closed".
 */
export function workersSummary(workers: Workers, words: Words): WorkersSummary {
    const sessions = workers.open.map((line) => line.session);
    const counts = URGENCY.map((kind) => {
        const doing = sessions.filter((one) => statusKind(one.status, one.unread) === kind);
        const background = addedUp(doing.map((one) => one.background));
        return {
            kind,
            count: doing.length,
            background,
            said: countSaid(kind, doing.length, words),
        };
    }).filter((one) => one.count > 0);
    const lead =
        sessions.length > 0
            ? `${sessions.length} ${words.workingFor.toLowerCase()}`
            : words.workingFor;
    const closed = workers.closed.length > 0 ? words.closed(workers.closed.length) : null;
    const parts = [...counts.map((one) => one.said), ...(closed === null ? [] : [closed])];
    return { lead, counts, closed, said: `${lead}: ${parts.join(", ")}` };
}

/** `count` sessions doing `kind`, in the style's words: "2 working", "1 needs you", "3 unread". */
function countSaid(kind: StatusKind, count: number, words: Words): string {
    if (kind === "waiting") {
        return `${count} ${count === 1 ? "needs" : "need"} you`;
    }
    if (kind === "unread") {
        return words.unread(count);
    }
    return `${count} ${(words.status[kind][0] ?? kind).toLowerCase()}`;
}

/** What the button that shows or hides those under a line names: "the 3 sessions that work under it". */
export function underSaid(count: number, words: Words): string {
    return count === 1
        ? `the ${words.session} that works under it`
        : `the ${count} ${words.session}s that work under it`;
}

/**
 * What of a repository decides whom the core lists under one of its sessions: each task's open
 * sessions, whom each works for, and how many closed ones it has. The header reads the list
 * again when it changes: a session started, closed, reopened, deleted or set to work for another,
 * or a task made or removed. The sidebar's order is left out: the list is the oldest first.
 */
export function workersKey(project: Project): string {
    return project.tasks
        .filter((task) => !task.archived)
        .map((task) => {
            const sessions = task.sessions.map((one) => `${one.id}>${one.parent ?? ""}`);
            return `${task.id}:${task.closed}:${sessions.join(",")}`;
        })
        .join(";");
}

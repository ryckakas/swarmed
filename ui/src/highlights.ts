// Highlights: a colour the user gives a task or a session so that it stands out where it is.
// Pure, so it can be tested; the sidebar's palette and the shortcut only act on it.

import type { Api } from "./api/client";
import { MARK_COLOURS, type MarkColour, type Project, type Task } from "./api/types";
import { writtenHotkey } from "./hotkeys";
import type { OrderTarget } from "./ordering";
import { findSession, findSessionHome } from "./tree";

/**
 * A row of the sidebar the user can highlight: a task or a session. Repositories and cloud
 * tasks are not.
 */
export type HighlightTarget = Extract<OrderTarget, { kind: "task" } | { kind: "session" }>;

/** What the palette offers, in order: each colour marks have, then none. */
export const HIGHLIGHT_CHOICES: (MarkColour | null)[] = [...MARK_COLOURS, null];

/** Whether a row can be highlighted: a task or a session. */
export function highlightable(target: OrderTarget): target is HighlightTarget {
    return target.kind === "task" || target.kind === "session";
}

/** Whether `one` and `other` are the same row. */
export function sameRow(one: HighlightTarget | null, other: HighlightTarget): boolean {
    if (one === null || one.kind !== other.kind) {
        return false;
    }
    if (one.kind === "session" && other.kind === "session") {
        return one.id === other.id;
    }
    return one.kind === "task" && other.kind === "task"
        ? one.project === other.project && one.task === other.task
        : false;
}

/** The open task `name` of repository `project`, if there is one. */
function openTask(projects: Project[], project: string, name: string) {
    const home = projects.find((one) => one.name === project);
    const task = home?.tasks.find((one) => one.name === name && !one.archived);
    return home && task ? { project: home, task } : undefined;
}

/** Where the row `target` stands for is in the tree, if it is. */
function placeOf(
    projects: Project[],
    target: HighlightTarget,
): { project: Project; task: Task } | undefined {
    if (target.kind === "task") {
        return openTask(projects, target.project, target.task);
    }
    const session = findSession(projects, target.id);
    return session && findSessionHome(projects, session);
}

/** The colour the row `target` stands for is highlighted in now; `undefined` if it is not in the tree. */
export function highlightNow(
    projects: Project[],
    target: HighlightTarget,
): MarkColour | null | undefined {
    if (target.kind === "session") {
        return findSession(projects, target.id)?.highlight;
    }
    return openTask(projects, target.project, target.task)?.task.highlight;
}

/** What the sidebar folds away, by the ids of what is folded or shown. */
export interface Folds {
    /** Repositories whose tasks are folded away. */
    folded: number[];
    /** Tasks whose sessions are folded away. */
    foldedTasks: number[];
    /** Repositories whose Done group, of the tasks put away, is unfolded. */
    doneShown: number[];
}

/**
 * The folds once the row `target` stands for is in view: its repository and the Done group
 * it is in unfold, and for a session its task does, so that the palette asked for by the
 * shortcut has a row to open at. The same object when nothing hides it.
 */
export function foldsShowing(folds: Folds, projects: Project[], target: HighlightTarget): Folds {
    const place = placeOf(projects, target);
    if (!place) {
        return folds;
    }
    const { project, task } = place;
    const hidesTask = target.kind === "session" && folds.foldedTasks.includes(task.id);
    const hidesProject = folds.folded.includes(project.id);
    const inDone = task.done_at !== null && !folds.doneShown.includes(project.id);
    if (!(hidesTask || hidesProject || inDone)) {
        return folds;
    }
    return {
        folded: hidesProject ? folds.folded.filter((id) => id !== project.id) : folds.folded,
        foldedTasks: hidesTask
            ? folds.foldedTasks.filter((id) => id !== task.id)
            : folds.foldedTasks,
        doneShown: inDone ? [...folds.doneShown, project.id] : folds.doneShown,
    };
}

/** A choice of the palette as it is written on its swatch: the colour's name, or None. */
export function choiceName(choice: MarkColour | null): string {
    const name = choice ?? "none";
    return name.charAt(0).toUpperCase() + name.slice(1);
}

/**
 * What a highlighted row adds to its name for those who cannot see the colour, such as
 * "highlighted blue"; nothing for a row that is not.
 */
export function highlightSaid(colour: MarkColour | null): string {
    return colour === null ? "" : `highlighted ${colour}`;
}

/** What the highlight button on row `name` is called, for those who cannot see it. */
export function highlightLabel(colour: MarkColour | null, name: string): string {
    return colour === null
        ? `Highlight ${name}`
        : `${name} is ${highlightSaid(colour)}: change or take off the highlight`;
}

/** What the highlight button says under the pointer, with the key that opens its palette too. */
export function highlightTitle(colour: MarkColour | null): string {
    const keys = writtenHotkey("highlight");
    return colour === null
        ? `Highlight in a colour, where it is. ${keys} on the focused row, or the open session.`
        : `${choiceName(colour)}. Press to change it or take it off, or ${keys} on the focused row.`;
}

/**
 * The swatch the keyboard goes to from swatch `at` of `count` for `key`: the arrows step and
 * wrap round, Home and End go to either end. `null` for any other key, which the palette
 * leaves alone: Enter and the space bar press the swatch, as on any button.
 */
export function paletteStep(at: number, key: string, count = HIGHLIGHT_CHOICES.length) {
    const steps: Record<string, number> = {
        ArrowLeft: -1,
        ArrowUp: -1,
        ArrowRight: 1,
        ArrowDown: 1,
    };
    if (key === "Home") {
        return 0;
    }
    if (key === "End") {
        return count - 1;
    }
    const step = steps[key];
    return step === undefined ? null : (at + step + count) % count;
}

/** Where the keyboard starts in the palette: on the row's colour, or on the first. */
export function paletteStart(colour: MarkColour | null): number {
    return colour === null ? 0 : Math.max(0, HIGHLIGHT_CHOICES.indexOf(colour));
}

/** Asks the core to highlight `target` in `colour`, or with `null` to take its highlight off. */
export function highlightTarget(
    api: Api,
    target: HighlightTarget,
    colour: MarkColour | null,
): Promise<void> {
    return target.kind === "task"
        ? api.taskHighlight(target.project, target.task, colour)
        : api.sessionHighlight(target.id, colour);
}

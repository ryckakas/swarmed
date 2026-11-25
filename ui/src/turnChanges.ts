// The Changes tab's choice between the whole task and one turn of the session on screen, and
// what it says about a turn. Pure, so it can be tested.

import type { SkippedFile, Turn } from "./api/types";
import { formatBytes } from "./usage";

/** A turn the user chose: of one session, by the number of the message that began it. */
export interface TurnChoice {
    session: number;
    message: number;
}

/** The picker's value for the whole task. */
export const WHOLE = "whole";

/**
 * The turn shown: the one chosen, while it is a turn of the session on screen that is still
 * kept. Anything else, a choice in another session or one of a turn no longer kept, is the
 * whole task.
 */
export function chosenTurn(
    choice: TurnChoice | null,
    session: number | null,
    turns: Turn[],
): Turn | null {
    if (choice === null || choice.session !== session) {
        return null;
    }
    return turns.find((turn) => turn.message === choice.message) ?? null;
}

/** What the picker offers: the whole task first, then the turns, the latest first. */
export function turnOptions(turns: Turn[], task: string): [value: string, label: string][] {
    const latestFirst = [...turns].reverse();
    return [
        [WHOLE, `Whole ${task}`],
        ...latestFirst.map((turn): [string, string] => [String(turn.message), turnLabel(turn)]),
    ];
}

/** A turn as the picker names it: the number of its message, counted from 1, and its words. */
export function turnLabel(turn: Turn): string {
    const words = turn.words === "" ? "(no words)" : turn.words;
    return `${turn.message + 1}. ${words}`;
}

/** The choice the picker's `value` stands for, in `session`. */
export function choiceOf(value: string, session: number | null): TurnChoice | null {
    const message = Number(value);
    if (value === WHOLE || session === null || !Number.isInteger(message)) {
        return null;
    }
    return { session, message };
}

/**
 * What is said above a turn's files: who else was at work during it, whether its end is
 * known, and the files too large to keep.
 */
export function turnNotes(turn: Turn, skipped: SkippedFile[], ended: boolean): string[] {
    const notes: string[] = [];
    if (turn.beside.length > 0) {
        notes.push(
            `Also at work during this turn: ${turn.beside.join(", ")}. What ${turn.beside.length === 1 ? "it" : "they"} changed meanwhile is in here too.`,
        );
    }
    if (!ended) {
        notes.push("This turn has no recorded end yet: these are its changes up to now.");
    }
    if (skipped.length > 0) {
        const files = skipped.map((file) => `${file.path} (${formatBytes(file.bytes)})`);
        notes.push(
            `Files over 10 MB that git does not track are not kept, so what this turn did to them is not shown: ${files.join(", ")}.`,
        );
    }
    return notes;
}

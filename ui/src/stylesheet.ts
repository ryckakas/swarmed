import type { Stylesheet } from "./api/types";

// The user's own stylesheet in the window. The core decides what is applied (`stylesheet.rs`):
// only what the user saved in Settings or accepted when told its file changed. Here is where
// it goes on the page, which notice the window shows of it, and how a change on disk is shown.

/** The id of the style element the user's stylesheet is drawn in. */
const ELEMENT_ID = "user-stylesheet";

/**
 * Keeps one style element in `page` holding `css`, the user's own stylesheet, as the last
 * element of the head: after every stylesheet of the app's own, the custom layout's included,
 * so that it wins where they disagree. Empty, the element is taken away.
 */
export function drawUserStylesheet(page: Document, css: string) {
    const held = page.getElementById(ELEMENT_ID);
    if (css === "") {
        held?.remove();
        return;
    }
    const element = held ?? Object.assign(page.createElement("style"), { id: ELEMENT_ID });
    if (element.textContent !== css) {
        element.textContent = css;
    }
    if (page.head.lastElementChild !== element) {
        page.head.appendChild(element);
    }
}

/** What the user put aside of the notices: the file text not to ask about, and the run's note. */
export interface StylesheetPutAside {
    /** The file, as it was when the user said not now to applying it. */
    file: string | null;
    /** Whether the note that this run leaves the stylesheet out was dismissed. */
    leftOut: boolean;
}

export const NOTHING_PUT_ASIDE: StylesheetPutAside = { file: null, leftOut: false };

/** The notice the window shows of the user's stylesheet, if any. */
export type StylesheetNotice =
    /** The file holds what the user has not accepted, shown to them before it is applied. */
    | { kind: "changed"; file: string; accepted: string }
    /** This run leaves the stylesheet out, though it is on: ⌥ was held as Swarmed started. */
    | { kind: "left_out" }
    | null;

/** Which notice to show of `sheet`, leaving out what the user put aside. */
export function stylesheetNotice(
    sheet: Stylesheet | null,
    aside: StylesheetPutAside,
): StylesheetNotice {
    if (!sheet?.on) {
        return null;
    }
    if (sheet.left_out) {
        return aside.leftOut ? null : { kind: "left_out" };
    }
    if (!sheet.waiting || sheet.file === null || sheet.file === aside.file) {
        return null;
    }
    return { kind: "changed", file: sheet.file, accepted: sheet.accepted };
}

/** A line of a change: one taken out, one put in, or where unchanged lines were left out. */
export interface ChangedLine {
    kind: "added" | "removed" | "gap";
    text: string;
}

/** Past this many line pairs, a change is shown as all of one taken out and all of the other put in. */
const MOST_COMPARED = 4_000_000;

/** The lines of `text`, none for none. */
function linesOf(text: string): string[] {
    return text === "" ? [] : text.replace(/\n$/, "").split("\n");
}

/** How long the longest run of lines in common is from each pair of places on: the LCS table. */
function commonFrom(before: string[], after: string[]): number[][] {
    const table = Array.from({ length: before.length + 1 }, () =>
        new Array<number>(after.length + 1).fill(0),
    );
    for (let at = before.length - 1; at >= 0; at--) {
        const row = table[at] ?? [];
        const next = table[at + 1] ?? [];
        for (let to = after.length - 1; to >= 0; to--) {
            row[to] =
                before[at] === after[to]
                    ? (next[to + 1] ?? 0) + 1
                    : Math.max(next[to] ?? 0, row[to + 1] ?? 0);
        }
    }
    return table;
}

/** A line of the two texts, unchanged, taken out or put in. */
interface WalkedLine {
    kind: "added" | "removed" | "same";
    text: string;
}

/** Every line of the two texts, in order, said as unchanged, taken out or put in. */
function walk(before: string[], after: string[]): WalkedLine[] {
    if (before.length * after.length > MOST_COMPARED) {
        return [
            ...before.map((text) => ({ kind: "removed" as const, text })),
            ...after.map((text) => ({ kind: "added" as const, text })),
        ];
    }
    const table = commonFrom(before, after);
    const out: WalkedLine[] = [];
    let at = 0;
    let to = 0;
    const common = (row: number, column: number) => table[row]?.[column] ?? 0;
    while (at < before.length || to < after.length) {
        const old = before[at];
        const now = after[to];
        if (old !== undefined && old === now) {
            out.push({ kind: "same", text: old });
            at++;
            to++;
        } else if (
            old !== undefined &&
            (now === undefined || common(at + 1, to) >= common(at, to + 1))
        ) {
            // What is taken out is said before what is put in its place.
            out.push({ kind: "removed", text: old });
            at++;
        } else {
            out.push({ kind: "added", text: now ?? "" });
            to++;
        }
    }
    return out;
}

/**
 * The lines that differ between `before` and `after`, in order, with a gap where unchanged lines
 * were left out between them: what the user is shown before a change on disk is applied.
 */
export function changedLines(before: string, after: string): ChangedLine[] {
    const lines: ChangedLine[] = [];
    let skipped = false;
    for (const line of walk(linesOf(before), linesOf(after))) {
        if (line.kind === "same") {
            skipped = true;
            continue;
        }
        if (skipped && lines.length > 0) {
            lines.push({ kind: "gap", text: "" });
        }
        skipped = false;
        lines.push({ kind: line.kind, text: line.text });
    }
    return lines;
}

/** How many lines a change put in and took out, in words. */
export function changeSaid(lines: ChangedLine[]): string {
    const added = lines.filter((line) => line.kind === "added").length;
    const removed = lines.filter((line) => line.kind === "removed").length;
    const count = (n: number, what: string) => `${n} ${n === 1 ? "line" : "lines"} ${what}`;
    const parts = [added > 0 && count(added, "added"), removed > 0 && count(removed, "removed")];
    const said = parts.filter((part): part is string => typeof part === "string");
    return said.length > 0 ? said.join(", ") : "only line breaks changed";
}

/**
 * Characters that change how text is drawn without showing themselves: zero-width ones and
 * those that turn the direction of text. In a change shown for the user's yes, each is drawn as
 * its code instead, so that nothing in a line hides what the line does.
 */
const UNSEEN = /[\u00AD\u061C\u180E\u200B-\u200F\u202A-\u202E\u2060-\u2064\u2066-\u206F\uFEFF]/g;

/** A line of a change as it is shown: every character that would not show, by its code. */
export function shownLine(text: string): string {
    return text.replace(
        UNSEEN,
        (unseen) =>
            `⟨U+${(unseen.codePointAt(0) ?? 0).toString(16).toUpperCase().padStart(4, "0")}⟩`,
    );
}

/** How long Apply waits after a change is shown, so that a click meant for the one before cannot take it. */
export const APPLY_AFTER_MS = 1500;

/** What the CSS box in Settings starts from: what is applied, never a change not yet accepted. */
export type StylesheetBox = { kind: "box"; text: string } | { kind: "too_large" };

/**
 * What Settings offers to edit. While the file holds a change the user has not accepted, the box
 * starts from what they accepted, and the change is shown apart, as in the window's notice: a
 * box that started from the file would have Save accept a change unseen. A file too large to be
 * read is no box at all, so that saving cannot replace what was never shown.
 */
export function stylesheetBox(sheet: Stylesheet): StylesheetBox {
    if (sheet.too_large) {
        return { kind: "too_large" };
    }
    return { kind: "box", text: sheet.waiting ? sheet.accepted : (sheet.file ?? "") };
}

/** Whether the box holds an edit of the user's own to save: typed, and not what it started from. */
export function mayKeep(started: string, typed: string | null): boolean {
    return typed !== null && typed !== started;
}

/**
 * Answers taken only in the order they were asked. The core reads the stylesheet off the main
 * thread and saves it on it, so an answer can arrive after one asked for later; taken, it would
 * put back what a save or an acceptance replaced.
 */
export function answersInOrder() {
    let asked = 0;
    let taken = 0;
    return {
        /** A number for the next answer. */
        ask: () => {
            asked += 1;
            return asked;
        },
        /** Whether answer `read` is newer than every answer taken, which it then is. */
        takes: (read: number) => {
            if (read <= taken) {
                return false;
            }
            taken = read;
            return true;
        },
    };
}

/** When the stylesheet's file last changed, as a date and a time of day; empty if unknown. */
export function changedAt(sheet: Stylesheet): string {
    if (sheet.changed_at === null) {
        return "";
    }
    return new Date(sheet.changed_at * 1000).toLocaleString(undefined, {
        dateStyle: "medium",
        timeStyle: "short",
    });
}

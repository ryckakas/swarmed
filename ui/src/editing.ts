// Keys the message box handles itself, where the system's own behaviour is not what is wanted
// while writing a prompt. Pure, so it can be tested.

/** What of a key press matters here. */
export interface EditPress {
    key: string;
    metaKey: boolean;
    ctrlKey: boolean;
    altKey: boolean;
    shiftKey: boolean;
}

/** What a key does to the text box: move or extend the selection, or empty the box. */
export type Edit =
    | { kind: "select"; start: number; end: number; direction: "forward" | "backward" | "none" }
    | { kind: "clear" };

/**
 * What `press` does in a box holding `value` with the selection from `start` to `end`, or `null`
 * to leave the key to the system.
 *
 * - Home and End go to the start and end of the line the caret is on (macOS would scroll
 *   instead); with Shift they select up to there.
 * - ⌘⌫ with the caret at the very start, where the system has nothing left to delete, empties
 *   the box.
 */
export function editFor(press: EditPress, value: string, start: number, end: number): Edit | null {
    if (press.ctrlKey || press.altKey) {
        return null;
    }
    if (press.metaKey) {
        const deletes = press.key === "Backspace" || press.key === "Delete";
        return deletes && start === 0 && end === 0 && value !== "" ? { kind: "clear" } : null;
    }
    if (press.key === "Home") {
        const lineStart = value.lastIndexOf("\n", start - 1) + 1;
        return press.shiftKey
            ? { kind: "select", start: lineStart, end, direction: "backward" }
            : { kind: "select", start: lineStart, end: lineStart, direction: "none" };
    }
    if (press.key === "End") {
        const newline = value.indexOf("\n", end);
        const lineEnd = newline === -1 ? value.length : newline;
        return press.shiftKey
            ? { kind: "select", start, end: lineEnd, direction: "forward" }
            : { kind: "select", start: lineEnd, end: lineEnd, direction: "none" };
    }
    return null;
}

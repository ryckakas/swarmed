import type { TerminalOpened, TerminalOutput } from "./api/types";

// What to draw when a terminal is opened again. Pure, so it can be tested.

/**
 * Whether a piece of output was printed after a replay that ends at `end`, and so is not in
 * it. Pieces are told apart by where they start, which the core counts; they are never cut,
 * since a replay always ends between two of them.
 */
export function comesAfter(output: TerminalOutput, end: number): boolean {
    return output.offset >= end;
}

/**
 * What a terminal shows once it is opened: what it had printed, then the pieces heard while
 * that was on its way that are not in it already. A shell that goes on printing while its
 * terminal is opened, a build for one, would otherwise lose those pieces or show them twice.
 */
export function redrawn(opened: TerminalOpened, early: TerminalOutput[]): string {
    const after = early.filter((output) => heard(output, opened) === "printed");
    return opened.replay + after.map((output) => output.data).join("");
}

/**
 * What a piece of output is to a terminal opened as `opened`: something it `printed` that is
 * still to be drawn, the word that what runs there has `ended`, or `nothing`. A piece of
 * another run is nothing to it: one that a new run took the place of may still be on its way
 * when the new one is opened, and would be drawn as the new one's, or end it on screen.
 */
export function heard(
    output: TerminalOutput,
    opened: TerminalOpened,
): "printed" | "ended" | "nothing" {
    if (output.run !== opened.run) {
        return "nothing";
    }
    if (output.ended) {
        return "ended";
    }
    return comesAfter(output, opened.end) ? "printed" : "nothing";
}

/** Whether the run opened as `opened` was heard to end among `early`, before it was drawn. */
export function endedEarly(opened: TerminalOpened, early: TerminalOutput[]): boolean {
    return early.some((output) => heard(output, opened) === "ended");
}

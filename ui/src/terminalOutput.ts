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
    const after = early.filter((output) => comesAfter(output, opened.end));
    return opened.replay + after.map((output) => output.data).join("");
}

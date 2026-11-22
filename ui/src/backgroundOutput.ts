// Following what a command an agent left running prints. Pure, so it can be tested.

import type { BackgroundOutput } from "./api/types";

/** What is said above the output: that only its latest part is shown, that it is gone, or why not. */
export type OutputNote = "cut" | "gone" | string;

/** What to do with a read of the output, given where the last read ended (`null`: none yet). */
export interface OutputStep {
    /** Whether what is shown has to go first: this read does not go on from the last. */
    reset: boolean;
    /** What to add to what is shown. */
    text: string;
    /** Where to read on from next time. */
    end: number | null;
    /** What to say above it, if anything new. */
    note: OutputNote | null;
}

/** What a read of the output changes about what is shown. */
export function outputStep(end: number | null, output: BackgroundOutput | null): OutputStep {
    if (output === null) {
        return { reset: false, text: "", end, note: "gone" };
    }
    const first = end === null;
    return {
        reset: !first && output.start !== end,
        text: output.text,
        end: output.end,
        note: first && output.start > 0 ? "cut" : null,
    };
}

/** The note in words. */
export function outputNote(note: OutputNote): string {
    if (note === "cut") {
        return "Only the latest part of what it printed is shown.";
    }
    if (note === "gone") {
        return "What it printed is gone: Claude Code clears it when it starts again.";
    }
    return note;
}

/** What the core said of a branch prefix: the branch it gives, or why it cannot be used. */
export type PrefixCheck = { prefix: string; branch: string } | { prefix: string; problem: string };

/** What went wrong, as words: the core's message, or a thrown error's. */
export function problemOf(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
}

/**
 * The prefix to keep when the field is left: what was typed, unless nothing was, it is what is
 * kept already, or the core has refused that very text. A refusal of something typed before
 * says nothing of it: the core checks again as it keeps it.
 */
export function prefixToSave(
    typed: string | null,
    kept: string,
    check: PrefixCheck | null,
): string | null {
    if (typed === null || typed === kept) {
        return null;
    }
    const refused = check !== null && check.prefix === typed && "problem" in check;
    return refused ? null : typed;
}

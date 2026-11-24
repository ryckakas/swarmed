// Quoting words of one of the agent's replies into the message box, with a comment under them.
// Pure, so it can be tested: what the window's selection is comes in as plain values, and the
// token that stands for the quote is made in `pastes.ts`, beside a large paste's.

/**
 * The words a selection quotes: what is selected, trimmed, when both of its ends lie in the
 * same reply of the agent's (`startReply` and `endReply` are that reply's key, or `null`
 * outside any reply that can be quoted from). A selection that runs over several items, or lies
 * in anything else, quotes nothing.
 */
export function quotedWords(
    selected: string,
    startReply: string | null,
    endReply: string | null,
): string | null {
    const words = selected.trim();
    return startReply !== null && startReply === endReply && words !== "" ? words : null;
}

/**
 * `value` with a quote's `token` put in at `at`, on a line of its own, and where the caret goes
 * then: at the start of the line under it, where the comment on it is written. Nothing already
 * in the box is written over.
 */
export function withQuote(value: string, at: number, token: string) {
    const before = value.slice(0, at);
    const after = value.slice(at);
    const opening = before === "" || before.endsWith("\n") ? "" : "\n";
    const closing = after.startsWith("\n") ? "" : "\n";
    return {
        value: `${before}${opening}${token}${closing}${after}`,
        caret: before.length + opening.length + token.length + 1,
    };
}

/** A rectangle as the window measures one, in its own coordinates. */
export interface Box {
    top: number;
    bottom: number;
    left: number;
    right: number;
}

/** Where the Quote button stands in the transcript's frame, from the frame's top left. */
export interface Spot {
    top: number;
    left: number;
}

/** Room left between the button and the words it quotes. */
const GAP = 4;

/**
 * Where the Quote button stands in the transcript's `frame` for a selection whose first line
 * is `first` and last line `last`: over the start of the selection, or under its end where
 * there is no room above, and inside the frame. The button is about `button` in size. `null`
 * while the selection is scrolled out of view, with nothing for the button to stand beside.
 */
export function quoteSpot(
    first: Box,
    last: Box,
    frame: Box,
    button: { width: number; height: number },
): Spot | null {
    if (last.bottom < frame.top || first.top > frame.bottom) {
        return null;
    }
    const above = first.top - frame.top - button.height - GAP;
    const fits = above >= 0;
    const top = fits ? above : last.bottom - frame.top + GAP;
    const left = (fits ? first.left : last.left) - frame.left;
    return {
        top: Math.max(0, Math.min(top, frame.bottom - frame.top - button.height)),
        left: Math.max(0, Math.min(left, frame.right - frame.left - button.width)),
    };
}

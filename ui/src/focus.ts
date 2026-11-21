// Who gets the keyboard when something new appears. Pure, so it can be tested.

/**
 * Whether something that just appeared may take the keyboard: only while nothing has it, which
 * a page says by naming its body, or nothing at all. A request that arrives while the user
 * types, in the message box, the terminal or anywhere else, must not take the next key press
 * for its answer.
 */
export function focusIsFree(active: unknown, body: unknown): boolean {
    return active === null || active === undefined || active === body;
}

/**
 * Whether a question the user asked for themselves, by sending a command such as `/model`, may
 * take the keyboard: while nothing has it, or while it is in a text box with nothing typed,
 * which is where sending the command left it. Anything typed is the user's next message.
 */
export function focusIsIdle(active: unknown, body: unknown): boolean {
    const box = active as { tagName?: unknown; value?: unknown } | null;
    return focusIsFree(active, body) || (box?.tagName === "TEXTAREA" && box.value === "");
}

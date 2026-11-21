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

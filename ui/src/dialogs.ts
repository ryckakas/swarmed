// What the one Dialog component decides, apart from drawing: where the keyboard starts and
// whether a click beside it closes it. Pure, so it can be tested.

/**
 * Where the keyboard starts when a dialog opens: the first element its content marks with
 * `data-dialog-start`, else the body's first field, else the main button, else the footer's
 * first button. A question whose action cannot be undone has no main button, so a stray Enter
 * takes its way out.
 */
export const DIALOG_STARTS = [
    "[data-dialog-start]",
    ".dialog-body :is(input:not([type='hidden']), textarea, select):not(:disabled)",
    ".dialog-foot .button-primary:not(:disabled)",
    ".dialog-foot button:not(:disabled)",
];

/** The element the keyboard starts on: what the first of the selectors to find anything finds. */
export function dialogStart<T>(find: (selector: string) => T | null): T | null {
    for (const selector of DIALOG_STARTS) {
        const found = find(selector);
        if (found) {
            return found;
        }
    }
    return null;
}

/** A click on a dialog, and what the dialog allows. */
export interface ClickBeside {
    /** Whether the press that began the click was on the dialog's own element, beside its box. */
    pressedBeside: boolean;
    /** Whether the click ended there too. */
    endedBeside: boolean;
    closeOnBackdrop: boolean;
    busy: boolean;
}

/**
 * Whether a click closes the dialog: only one that began and ended beside it, where the
 * dialog lets that close it, and never while an action runs. A click that began inside, such
 * as text selected past the edge, is no click beside it.
 */
export function closesBeside(click: ClickBeside): boolean {
    return click.pressedBeside && click.endedBeside && click.closeOnBackdrop && !click.busy;
}

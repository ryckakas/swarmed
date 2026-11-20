// What the row of the user's own buttons shows. Pure, so it can be tested.

import type { Action } from "./api/types";

/** How many actions are shown as buttons; the rest are in a menu. */
export const SHOWN_ACTIONS = 6;

/** The actions that get a button of their own, and those kept in the menu after them. */
export function splitActions(actions: Action[], shown = SHOWN_ACTIONS): [Action[], Action[]] {
    // A menu holding one thing takes the room of the button it saves.
    const fits = actions.length <= shown + 1 ? actions.length : shown;
    return [actions.slice(0, fits), actions.slice(fits)];
}

/** What an action does, for its tooltip: the start of its prompt or its command. */
export function actionTooltip(action: Action): string {
    const [kind, text] =
        action.command === ""
            ? ["Sends the agent", action.prompt]
            : ["Runs in the terminal", action.command];
    const first = text.split("\n")[0] ?? "";
    const cut = first.length > 120 || text.includes("\n");
    return `${kind}: ${first.slice(0, 120)}${cut ? "…" : ""}`;
}

/** What an action's button or menu line says: its sign, if it has one, and its name. */
export function actionLabel(action: Action): string {
    return action.icon === "" ? action.name : `${action.icon} ${action.name}`;
}

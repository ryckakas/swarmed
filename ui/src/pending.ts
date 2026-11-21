import { questionsOf, SKIPPED } from "./questions";
import type { Item } from "./transcript";

// Which request the shortcuts for allowing and denying answer. Pure, so it can be tested.

/** A request a shortcut answers, and what the agent is told if the answer is no. */
export interface ShortcutRequest {
    requestId: string;
    refusal: string;
}

const DECLINED = "The user declined this action.";

/**
 * The request the allow shortcut, or with `allow` false the deny shortcut, would answer: the
 * last one still open in the transcript. Questions the agent puts to the user cannot be
 * allowed by a key, since allowing them without answers tells the agent nothing; denying
 * them is what Skip on the question does, and says the same.
 */
export function shortcutRequest(items: Item[], allow: boolean): ShortcutRequest | null {
    const request = items.findLast((item) => item.kind === "permission");
    if (request?.kind !== "permission") {
        return null;
    }
    const asks = questionsOf(request.toolName, request.input) !== null;
    if (asks && allow) {
        return null;
    }
    return { requestId: request.requestId, refusal: asks ? SKIPPED : DECLINED };
}

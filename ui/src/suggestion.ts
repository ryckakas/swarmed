// The next message an agent suggests, as far as the conversation still stands where it left it.
// Pure, so it can be tested.

import type { AgentEvent } from "./api/types";

/** Events that move the conversation on, after which an earlier suggestion no longer fits. */
const MOVES_ON = new Set<AgentEvent["type"]>([
    "user_message",
    "text",
    "tool_use",
    "permission_request",
    "turn_done",
    "exited",
]);

/**
 * The message the agent last suggested, or `null` if there is none or the conversation has moved
 * on since. A suggestion that arrives after the user already wrote something else is dropped by
 * the end of that turn.
 */
export function suggestionOf(events: AgentEvent[] | undefined): string | null {
    for (let index = (events?.length ?? 0) - 1; index >= 0; index--) {
        const event = events?.[index];
        if (event?.type === "suggestion") {
            return event.text;
        }
        if (event && MOVES_ON.has(event.type)) {
            return null;
        }
    }
    return null;
}

/** Whether `key` on a box holding `value` takes `suggestion` into the box. */
export function takesSuggestion(
    press: { key: string; shiftKey: boolean },
    value: string,
    suggestion: string | null | undefined,
): suggestion is string {
    return press.key === "Tab" && !press.shiftKey && value === "" && Boolean(suggestion);
}

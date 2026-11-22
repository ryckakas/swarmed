// What the queue of waiting messages says about itself. Pure, so it can be tested.

import type { QueuedMessage } from "./api/types";
import { type Words, wordsFor } from "./wording";

/**
 * The queue's heading: how many messages wait, and when the first goes. While the first is
 * open for editing (`editing`), the queue sends nothing by itself, so it says that instead.
 */
export function queueHeading(
    queue: QueuedMessage[],
    busy: boolean,
    editing: number | null,
    words: Words = wordsFor("plain"),
): string {
    const count = queue.length === 1 ? "1 message waiting" : `${queue.length} messages waiting`;
    if (editing !== null && queue[0]?.id === editing) {
        return `${count}, the first sent once you finish editing it`;
    }
    return busy ? `${count}, sent when the ${words.terms.agent} is done` : count;
}

/** Which message is open for editing once the one with `id` opened or closed its edit box. */
export function editingAfter(was: number | null, id: number, open: boolean): number | null {
    if (open) {
        return id;
    }
    return was === id ? null : was;
}

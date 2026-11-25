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

/** How many changes of session `sessionId`'s queue were heard, by session. */
export function changesHeard(heard: Record<number, number>, sessionId: number): number {
    return heard[sessionId] ?? 0;
}

/** `heard`, with one more change of session `sessionId`'s queue heard. */
export function oneMoreHeard(
    heard: Record<number, number>,
    sessionId: number,
): Record<number, number> {
    return { ...heard, [sessionId]: changesHeard(heard, sessionId) + 1 };
}

/**
 * The queues once the core answered that session `sessionId`'s queue holds `messages`, asked
 * when `asked` changes of it had been heard: as they were if one was heard since. Every change
 * is published, so that one is as new as the answer or newer, and an answer can reach the
 * window after an event published later.
 */
export function queuesAnswered(
    queues: Record<number, QueuedMessage[]>,
    heard: Record<number, number>,
    answer: { sessionId: number; messages: QueuedMessage[]; asked: number },
): Record<number, QueuedMessage[]> {
    const { sessionId, messages, asked } = answer;
    return changesHeard(heard, sessionId) === asked ? { ...queues, [sessionId]: messages } : queues;
}

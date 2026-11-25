import { describe, expect, it } from "vitest";
import { changesHeard, editingAfter, oneMoreHeard, queueHeading, queuesAnswered } from "./queue";

const queue = [
    { id: 1, text: "first" },
    { id: 2, text: "second" },
];

describe("queueHeading", () => {
    it("says when the waiting messages go", () => {
        expect(queueHeading(queue, true, null)).toBe(
            "2 messages waiting, sent when the agent is done",
        );
        expect(queueHeading(queue.slice(0, 1), false, null)).toBe("1 message waiting");
    });

    it("says the first waits for its edit while it is open", () => {
        expect(queueHeading(queue, true, 1)).toBe(
            "2 messages waiting, the first sent once you finish editing it",
        );
        // One further back holds nothing up.
        expect(queueHeading(queue, true, 2)).toBe(
            "2 messages waiting, sent when the agent is done",
        );
    });
});

describe("editingAfter", () => {
    it("follows the box that opened, and forgets it once it closes", () => {
        expect(editingAfter(null, 1, true)).toBe(1);
        expect(editingAfter(1, 1, false)).toBeNull();
        // A box that closes after another opened leaves that one open.
        expect(editingAfter(2, 1, false)).toBe(2);
    });
});

describe("an answer about a queue", () => {
    const waiting = { 7: queue };

    it("is taken while no change of the queue was heard since it was asked", () => {
        const answer = { sessionId: 7, messages: [], asked: 2 };
        expect(queuesAnswered(waiting, { 7: 2 }, answer)).toEqual({ 7: [] });
    });

    it("is passed over once a change was heard since, which is as new or newer", () => {
        const answer = { sessionId: 7, messages: queue, asked: 2 };
        const now = { 7: [] };
        expect(queuesAnswered(now, { 7: 3 }, answer)).toBe(now);
    });

    it("counts each session's changes apart, from none", () => {
        expect(changesHeard({}, 7)).toBe(0);
        const heard = oneMoreHeard(oneMoreHeard({ 8: 4 }, 7), 7);
        expect(heard).toEqual({ 7: 2, 8: 4 });
        expect(queuesAnswered({}, heard, { sessionId: 8, messages: queue, asked: 4 })).toEqual({
            8: queue,
        });
    });
});

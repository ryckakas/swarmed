import { describe, expect, it } from "vitest";
import { editingAfter, queueHeading } from "./queue";

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

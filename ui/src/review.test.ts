import { describe, expect, it } from "vitest";
import { commentKey, madeOn, type ReviewComment, reviewMessage, withComment } from "./review";

const comment = (path: string, line: number, text: string, more = {}): ReviewComment => ({
    path,
    line,
    side: "new",
    code: `  line ${line}  `,
    text,
    ...more,
});

describe("madeOn", () => {
    it("shows a comment only on the line it was made on", () => {
        const made = comment("a.ts", 3, "why?");
        expect(madeOn(made, "  line 3  ")).toBe(made);
        // The same place in another diff of the file, where other code stands.
        expect(madeOn(made, "something else")).toBeUndefined();
        expect(madeOn(undefined, "  line 3  ")).toBeUndefined();
    });
});

describe("withComment", () => {
    it("adds a comment, and replaces the one on the same line", () => {
        const one = withComment([], comment("a.ts", 3, "why?"));
        expect(one).toHaveLength(1);
        const changed = withComment(one, comment("a.ts", 3, "why not?"));
        expect(changed.map((c) => c.text)).toEqual(["why not?"]);
    });

    it("takes a comment away when its text is emptied", () => {
        const one = withComment([], comment("a.ts", 3, "why?"));
        expect(withComment(one, comment("a.ts", 3, "  "))).toEqual([]);
    });

    it("tells the old side of a line from the new one", () => {
        const both = withComment(
            withComment([], comment("a.ts", 3, "new")),
            comment("a.ts", 3, "old", { side: "old" }),
        );
        expect(both).toHaveLength(2);
        expect(commentKey(both[0] as ReviewComment)).not.toBe(commentKey(both[1] as ReviewComment));
    });
});

describe("reviewMessage", () => {
    it("lists the comments by file and line, each under its code", () => {
        const message = reviewMessage([
            comment("b.ts", 9, "Rename this."),
            comment("a.ts", 12, "Handle null."),
            comment("a.ts", 4, "Why removed?", { side: "old" }),
        ]);
        const order = ["a.ts:4 (a line you removed)", "a.ts:12", "b.ts:9"].map((where) =>
            message.indexOf(where),
        );
        expect(order.every((at) => at >= 0)).toBe(true);
        expect([...order].sort((a, b) => a - b)).toEqual(order);
        expect(message).toContain("a.ts:12\n> line 12\nHandle null.");
        expect(message.endsWith("tell me what you changed.")).toBe(true);
    });

    it("is nothing without comments", () => {
        expect(reviewMessage([])).toBe("");
    });
});

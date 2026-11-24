import { describe, expect, it } from "vitest";
import type { AgentEvent } from "./api/types";
import {
    type Browsing,
    browse,
    caretAfter,
    type RecallPress,
    recallDirection,
    sentMessages,
} from "./recall";
import type { Draft } from "./sending";

const press = (key: string, modifiers: Partial<RecallPress> = {}): RecallPress => ({
    key,
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    ...modifiers,
});

const said = (text: string): AgentEvent => ({ type: "user_message", text });
const draft = (text: string, files: string[] = []): Draft => ({ text, files, images: [] });

describe("recallDirection", () => {
    const TEXT = "first line\nsecond line\nthird";

    it("takes ↑ only at the very start and ↓ only at the very end", () => {
        expect(recallDirection(press("ArrowUp"), TEXT, 0, 0)).toBe("older");
        expect(recallDirection(press("ArrowUp"), TEXT, 14, 14)).toBeNull();
        expect(recallDirection(press("ArrowDown"), TEXT, TEXT.length, TEXT.length)).toBe("newer");
        expect(recallDirection(press("ArrowDown"), TEXT, 14, 14)).toBeNull();
        expect(recallDirection(press("ArrowDown"), TEXT, 0, 0)).toBeNull();
        // An empty box is both.
        expect(recallDirection(press("ArrowUp"), "", 0, 0)).toBe("older");
        expect(recallDirection(press("ArrowDown"), "", 0, 0)).toBe("newer");
    });

    it("leaves the arrows to move the caret within a line the box may have wrapped", () => {
        // One line of text, which a narrow box shows as several rows.
        const long = "a prompt long enough to wrap ".repeat(12);
        expect(recallDirection(press("ArrowUp"), long, 4, 4)).toBeNull();
        expect(recallDirection(press("ArrowUp"), long, long.length, long.length)).toBeNull();
        expect(recallDirection(press("ArrowDown"), long, 200, 200)).toBeNull();
        expect(recallDirection(press("ArrowDown"), long, 0, 0)).toBeNull();
    });

    it("leaves a selection, a modifier and other keys alone", () => {
        expect(recallDirection(press("ArrowUp"), "one", 0, 3)).toBeNull();
        expect(recallDirection(press("ArrowUp", { shiftKey: true }), "one", 0, 0)).toBeNull();
        expect(recallDirection(press("ArrowUp", { metaKey: true }), "one", 0, 0)).toBeNull();
        expect(recallDirection(press("ArrowUp", { altKey: true }), "one", 0, 0)).toBeNull();
        expect(recallDirection(press("ArrowLeft"), "one", 0, 0)).toBeNull();
    });
});

describe("sentMessages", () => {
    it("lists what the user typed, newest first, files apart and repeats once", () => {
        const events: AgentEvent[] = [
            said("first"),
            { type: "text", text: "an answer" },
            said("/code-review 123"),
            said("/code-review 123"),
            said("look at these\n\nAttached files:\n- /tmp/a.png\n- /tmp/b.md"),
            said("first"),
        ];
        expect(sentMessages(events)).toEqual([
            draft("first"),
            draft("look at these", ["/tmp/a.png", "/tmp/b.md"]),
            draft("/code-review 123"),
            draft("first"),
        ]);
    });

    it("leaves out what another session left", () => {
        const left =
            'Message from session 4 ("Docs"), another agent session of the same user:\n\nDone.';
        expect(sentMessages([said("mine"), said(left)])).toEqual([draft("mine")]);
    });

    it("leaves out the first message of a session another one started", () => {
        const events: AgentEvent[] = [
            { type: "text", text: "Starting." },
            said("Check the docs.\n\n(You were started by session 2, another agent session…)"),
            { type: "text", text: "Done." },
            said("and the README"),
        ];
        expect(sentMessages(events, true)).toEqual([draft("and the README")]);
        expect(sentMessages(events)).toHaveLength(2);
    });
});

describe("browse", () => {
    const SENT = [draft("newest"), draft("middle", ["/m.txt"]), draft("oldest")];
    const step = (browsing: Browsing | null, older: boolean, now: Draft) =>
        browse(browsing, older ? "older" : "newer", now, () => SENT);

    it("goes back through the messages and forward again to the draft as it was", () => {
        const typed = draft("half written", ["/x.png"]);
        const one = step(null, true, typed);
        expect(one?.shown).toEqual(draft("newest"));
        const two = step(one, true, draft("newest"));
        expect(two?.shown).toEqual(draft("middle", ["/m.txt"]));
        const three = step(two, true, draft("middle", ["/m.txt"]));
        expect(three?.shown).toEqual(draft("oldest"));
        expect(step(three, true, draft("oldest"))).toBeNull();

        const toMiddle = step(three, false, draft("oldest"));
        expect(toMiddle?.shown).toEqual(draft("middle", ["/m.txt"]));
        const toNewest = step(toMiddle, false, draft("middle", ["/m.txt"]));
        expect(toNewest?.shown).toEqual(draft("newest"));
        const home = step(toNewest, false, draft("newest"));
        expect(home?.shown).toBe(typed);
        expect(step(home, false, typed)).toBeNull();
    });

    it("ends when the box is typed in, and ↑ starts again from the newest", () => {
        const two = step(step(null, true, draft("")), true, draft("newest"));
        expect(two?.shown).toEqual(draft("middle", ["/m.txt"]));
        // The recalled message was changed: it is the draft now.
        const edited = draft("middle, changed", ["/m.txt"]);
        expect(step(two, false, edited)).toBeNull();
        const again = step(two, true, edited);
        expect(again).toMatchObject({ at: 0, shown: draft("newest"), draft: edited });
    });

    it("brings back a draft's pasted images with it, and ends when an image is pasted", () => {
        const image = { key: 1, format: "png" as const, data: "iVBORw0KGgo=" };
        const typed = { ...draft("look"), images: [image] };
        const one = step(null, true, typed);
        expect(step(one, false, draft("newest"))?.shown).toBe(typed);
        // An image pasted into a message brought back makes it a draft of its own.
        const pasted = { ...draft("newest"), images: [image] };
        expect(step(one, false, pasted)).toBeNull();
    });

    it("does nothing with ↓ before ↑, or with nothing sent", () => {
        expect(step(null, false, draft("typed"))).toBeNull();
        expect(browse(null, "older", draft(""), () => [])).toBeNull();
    });

    it("shows each message as typed again, and keeps it so", () => {
        const sent = () => [draft("<b>newest</b>"), draft("<b>oldest</b>")];
        let made = 0;
        const typedAgain = (message: Draft) => {
            made += 1;
            return { ...message, text: message.text.replaceAll(/<\/?b>/g, "") };
        };
        const one = browse(null, "older", draft(""), sent, typedAgain);
        expect(one?.shown).toEqual(draft("newest"));
        const two = browse(one, "older", draft("newest"), sent, typedAgain);
        expect(two?.shown).toEqual(draft("oldest"));
        const back = browse(two, "newer", draft("oldest"), sent, typedAgain);
        expect(back?.shown).toEqual(draft("newest"));
        // Shown again, the newest is made from what it was made into the first time.
        expect(made).toBe(3);
        expect(back?.sent).toEqual([draft("newest"), draft("oldest")]);
    });

    it("works the list out only when going back starts", () => {
        let made = 0;
        const sent = () => {
            made += 1;
            return SENT;
        };
        const one = browse(null, "older", draft(""), sent);
        browse(one, "older", draft("newest"), sent);
        expect(made).toBe(1);
    });
});

describe("caretAfter", () => {
    it("leaves the caret where the same key goes on working", () => {
        expect(caretAfter("older", "one\ntwo")).toBe(0);
        expect(caretAfter("older", "one")).toBe(0);
        expect(caretAfter("newer", "one\ntwo")).toBe(7);
        // Where it leaves the caret, the same key is taken again.
        const text = "one\ntwo";
        const back = caretAfter("older", text);
        expect(recallDirection(press("ArrowUp"), text, back, back)).toBe("older");
        const on = caretAfter("newer", text);
        expect(recallDirection(press("ArrowDown"), text, on, on)).toBe("newer");
    });
});

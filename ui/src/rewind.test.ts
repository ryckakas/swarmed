import { describe, expect, it } from "vitest";
import type { AgentEvent } from "./api/types";
import {
    commitsSaid,
    draftOf,
    droppedRanges,
    messageNamed,
    messageSent,
    messageWords,
    rewindChoices,
    rewoundSaid,
    withEdited,
} from "./rewind";
import { NO_DRAFT } from "./sending";

const user = (text: string): AgentEvent => ({ type: "user_message", text });
const rewound = (to: number): AgentEvent => ({ type: "rewound", to });

describe("taking a session back", () => {
    it("drops from the message gone back to before up to the rewind, as the core does", () => {
        const events = [user("one"), user("two"), rewound(1), user("three"), rewound(9)];
        // A rewind to a message never sent drops nothing.
        expect(droppedRanges(events)).toEqual([[1, 2]]);
    });

    it("offers the user's messages still in the conversation, the latest first", () => {
        const peer = user(
            'Message from session 4 ("other"), another agent session of the same user:\n\nhi',
        );
        const events = [user("one"), peer, user("two"), rewound(2), user("three")];
        expect(rewindChoices(events)).toEqual([
            { number: 3, text: "three" },
            { number: 0, text: "one" },
        ]);
    });

    it("puts a message back as it was typed, its files apart", () => {
        const sent = "Look at this\n\nAttached files:\n- /tmp/a.png";
        expect(messageSent([user("zero"), user(sent)], 1)).toBe(sent);
        expect(messageSent([user("zero")], 3)).toBeNull();
        expect(draftOf(sent)).toEqual({ text: "Look at this", files: ["/tmp/a.png"], images: [] });
        expect(messageWords(sent)).toBe("Look at this");
    });

    it("loses nothing already typed in the box", () => {
        const message = { text: "edit me", files: ["/a"], images: [] };
        expect(withEdited(NO_DRAFT, message)).toEqual(message);
        const typed = { text: "half a thought", files: ["/a", "/b"], images: [] };
        expect(withEdited(typed, message)).toEqual({
            text: "edit me\n\nhalf a thought",
            files: ["/a", "/b"],
            images: [],
        });
    });

    it("says where it went back to, by the message's start, and what of the files", () => {
        expect(messageNamed(`\n${"a".repeat(80)}\nmore`)).toBe(`${"a".repeat(59)}…`);
        expect(rewoundSaid("Fix it", false, false)).toBe("Taken back to before “Fix it”.");
        expect(rewoundSaid("Fix it", true, false)).toBe(
            "Taken back, files too, to before “Fix it”.",
        );
        expect(rewoundSaid("", true, true)).toBe(
            "Taken back to before the message. The files were put back since.",
        );
        expect(commitsSaid(0)).toBeNull();
        expect(commitsSaid(null)).toBeNull();
        expect(commitsSaid(1)).toBe(
            "1 commit made since stays on the branch: only the files went back.",
        );
        expect(commitsSaid(3)).toContain("3 commits made since stay");
    });
});

import { describe, expect, it } from "vitest";
import type { Reachable } from "./api/types";
import { mentionedPieces } from "./githubMentions";
import {
    fromManyRepositories,
    MENTION_LIMIT,
    mentionedApart,
    mentionedIn,
    mentionMatches,
    mentionQuery,
    mentionSpans,
    mentionsAgain,
    mentionsAsWords,
    mentionToken,
    moreToFind,
    placeOf,
    splitMentions,
    tokenLabel,
    withMention,
    withMentions,
} from "./mentions";
import { tokenToRemove } from "./pastes";
import { splitSent } from "./transcript";

function reachable(id: number, title: string, more: Partial<Reachable> = {}): Reachable {
    return {
        id,
        title,
        task: "login",
        project: "storefront",
        chat: false,
        same_task: false,
        status: "idle",
        ...more,
    };
}

describe("the token", () => {
    it("names the session by id, with its title on one line and cut at a word", () => {
        expect(mentionToken(42, "Fix the login form")).toBe("[@42 Fix the login form]");
        expect(mentionToken(7, "  ")).toBe("[@7]");
        expect(tokenLabel("two\nlines [and] brackets")).toBe("two lines [and) brackets");
        const long = "Make the checkout remember the last address the customer used";
        expect(tokenLabel(long)).toBe("Make the checkout remember the last…");
    });

    it("is found again in the text, each session once, in order", () => {
        const text = "see [@4 One] and [@12 Two], then [@4 One] again; [@x] is words";
        expect(mentionedIn(text)).toEqual([4, 12]);
        expect(mentionSpans("a [@4 One] b")).toEqual([{ start: 2, end: 10 }]);
    });

    it("is taken out whole by Backspace after it and Delete before it", () => {
        const value = "ask [@4 One] now";
        expect(tokenToRemove("Backspace", value, 12, 12)).toEqual({ start: 4, end: 12 });
        expect(tokenToRemove("Delete", value, 4, 4)).toEqual({ start: 4, end: 12 });
        expect(tokenToRemove("Backspace", value, 11, 11)).toBeNull();
    });
});

describe("what has been typed of a mention", () => {
    it("runs from an @ at the start of a word to the caret, spaces and all", () => {
        expect(mentionQuery("@", 1)).toEqual({ start: 0, query: "" });
        expect(mentionQuery("ask @fix log", 12)).toEqual({ start: 4, query: "fix log" });
        expect(mentionQuery("(@fix)", 5)).toEqual({ start: 1, query: "fix" });
    });

    it("is none for an @ inside a word, after a space, on another line, or in a token", () => {
        expect(mentionQuery("me@example.com", 14)).toBeNull();
        expect(mentionQuery("@ then words", 12)).toBeNull();
        expect(mentionQuery("@fix\nmore", 9)).toBeNull();
        expect(mentionQuery("[@4 One] ", 9)).toBeNull();
        expect(mentionQuery("nothing here", 5)).toBeNull();
        expect(mentionQuery(`@${"x".repeat(41)}`, 42)).toBeNull();
    });
});

describe("the sessions offered", () => {
    const sessions = [
        reachable(5, "Beside me", { same_task: true }),
        reachable(9, "Fix the login form", { task: "auth" }),
        reachable(3, "Checkout totals", { task: "checkout" }),
    ];

    it("are only the session's own task's until something is typed", () => {
        expect(mentionMatches(sessions, "", true).map((s) => s.id)).toEqual([5]);
        expect(moreToFind(sessions, [sessions[0] as Reachable], "")).toBe(true);
        // A new task has no task of its own yet: the latest few.
        expect(mentionMatches(sessions, "", false).map((s) => s.id)).toEqual([5, 9, 3]);
    });

    it("narrow to those with every word typed in their title, task, repository or id", () => {
        expect(mentionMatches(sessions, "LOGIN fix", true).map((s) => s.id)).toEqual([9]);
        expect(mentionMatches(sessions, "checkout", true).map((s) => s.id)).toEqual([3]);
        expect(mentionMatches(sessions, "9", true).map((s) => s.id)).toEqual([9]);
        expect(mentionMatches(sessions, "nothing", true)).toEqual([]);
        expect(moreToFind(sessions, [], "nothing")).toBe(false);
        const many = Array.from({ length: 20 }, (_, at) => reachable(at, `Session ${at}`));
        expect(mentionMatches(many, "session", true)).toHaveLength(MENTION_LIMIT);
    });

    it("say where each works, with the repository where there are several", () => {
        const chat = reachable(2, "Design talk", { chat: true, task: "design", project: ".chats" });
        const other = reachable(8, "Lint", { project: "bonsai-lint", task: "rules" });
        expect(fromManyRepositories([...sessions, chat])).toBe(false);
        expect(fromManyRepositories([...sessions, other])).toBe(true);
        expect(placeOf(chat, true)).toBe("chat design");
        expect(placeOf(other, true)).toBe("bonsai-lint / rules");
        expect(placeOf(other, false)).toBe("rules");
    });
});

describe("choosing a session", () => {
    it("puts its token in place of what was typed of it, with a space after", () => {
        const chosen = withMention("ask @fix please", 4, 8, { id: 9, title: "Fix login" });
        expect(chosen).toEqual({ value: "ask [@9 Fix login] please", caret: 19 });
        const atEnd = withMention("@fi", 0, 3, { id: 9, title: "Fix login" });
        expect(atEnd).toEqual({ value: "[@9 Fix login] ", caret: 15 });
    });
});

describe("reading a sent message back", () => {
    const listed = withMentions("do what [@9 Fix login] did", [
        { id: 9, title: "Fix login" },
        { id: 3, title: "" },
    ]);

    it("finds the sessions the core listed under the words", () => {
        expect(listed).toBe(
            'do what [@9 Fix login] did\n\nSessions mentioned, to read with the read_session tool:\n- session 9 ("Fix login")\n- session 3 ("untitled")',
        );
        expect(splitMentions(listed)).toEqual({
            text: "do what [@9 Fix login] did",
            sessions: [
                { id: 9, title: "Fix login" },
                { id: 3, title: "untitled" },
            ],
        });
    });

    it("reads a list with what was said in each, leaving what was said out", () => {
        const message = [
            "see this",
            "",
            "Sessions mentioned, with what was last said in each:",
            '- session 4 ("Greeting"):',
            "<session_4>",
            "User: hello",
            '- session 99 ("not one")',
            "</session_4>",
        ].join("\n");
        expect(splitMentions(message)).toEqual({
            text: "see this",
            sessions: [{ id: 4, title: "Greeting" }],
        });
    });

    it("leaves a message alone whose list is not one", () => {
        const typed = "Sessions mentioned, to read with the read_session tool:\nnot a list";
        expect(splitMentions(typed)).toEqual({ text: typed, sessions: [] });
        const unclosed =
            'x\n\nSessions mentioned, with what was last said in each:\n- session 4 ("G"):\n<session_4>\nhi';
        expect(splitMentions(unclosed).sessions).toEqual([]);
    });

    it("takes the files off first, then the sessions", () => {
        const message = `${listed}\n\nAttached files:\n- /tmp/a.txt`;
        expect(splitSent(message)).toEqual({
            text: "do what [@9 Fix login] did",
            files: ["/tmp/a.txt"],
            sessions: [
                { id: 9, title: "Fix login" },
                { id: 3, title: "untitled" },
            ],
            github: [],
            elements: [],
        });
    });

    it("brings each session back as a token, at the end where the words named it nowhere", () => {
        expect(mentionsAgain(listed)).toBe("do what [@9 Fix login] did [@3 untitled]");
        expect(mentionsAgain(withMentions("", [{ id: 3, title: "Three" }]))).toBe("[@3 Three]");
        expect(mentionsAgain("plain words")).toBe("plain words");
    });

    it("names a task by the words of a token's title", () => {
        expect(mentionsAsWords("carry on from [@3 Fix login] now")).toBe(
            "carry on from Fix login now",
        );
        expect(mentionsAsWords("ask [@3]")).toBe("ask session 3");
    });

    it("draws the tokens of listed sessions as sessions, and others as words", () => {
        const sessions = [{ id: 9, title: "Fix login" }];
        const text = "do what [@9 Fix login] did, not [@5 Five]";
        expect(mentionedPieces(text, sessions, [])).toEqual([
            "do what ",
            { session: { id: 9, title: "Fix login" } },
            " did, not [@5 Five]",
        ]);
        expect(mentionedApart(text, [...sessions, { id: 3, title: "Three" }])).toEqual([
            { id: 3, title: "Three" },
        ]);
    });
});

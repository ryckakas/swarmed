import { describe, expect, it } from "vitest";
import type { GitHubItem } from "./api/types";
import {
    asksGitHub,
    GITHUB_LIMIT,
    type GitHubMentioned,
    githubApart,
    githubIn,
    githubMatches,
    githubOf,
    githubQuery,
    githubSaid,
    githubSpans,
    githubToken,
    MENTIONED_ON_GITHUB,
    mentionedPieces,
    splitGitHub,
    splitMentioned,
    typedAgain,
    withGitHub,
    withGitHubList,
} from "./githubMentions";
import { mentionsAsWords, signAt, withMentions } from "./mentions";
import { tokenToRemove } from "./pastes";
import { sentMessages } from "./recall";
import { splitSent } from "./transcript";

function pull(number: number, title: string, more: Partial<GitHubItem> = {}): GitHubItem {
    return {
        kind: "pull",
        number,
        title,
        state: "open",
        url: `https://github.com/o/r/pull/${number}`,
        head: "fix-login",
        base: "main",
        updated_at: "2026-11-20T10:00:00Z",
        ...more,
    };
}

function issue(number: number, title: string, more: Partial<GitHubItem> = {}): GitHubItem {
    return {
        ...pull(number, title),
        kind: "issue",
        url: `https://github.com/o/r/issues/${number}`,
        head: null,
        base: null,
        ...more,
    };
}

/** An item as a sent message names it. */
function named(item: GitHubItem): GitHubMentioned {
    const { updated_at: _changed, ...rest } = item;
    return rest;
}

/** As the core writes it (`with_github` in `github_mentions.rs`). */
const LISTED = [
    "fix what [#142 Fix the login form] left",
    "",
    MENTIONED_ON_GITHUB,
    '- pull request #142 ("Fix the "login" form", merged, fix-login into main): https://github.com/o/r/pull/142',
    '- issue #12 ("Login loops", closed): https://github.com/o/r/issues/12',
].join("\n");

describe("typing #", () => {
    it("opens at the start of the box or a word, never in one, nor before a space", () => {
        expect(githubQuery("#", 1)).toEqual({ start: 0, query: "" });
        expect(githubQuery("see #14", 7)).toEqual({ start: 4, query: "14" });
        expect(githubQuery("see #login form", 15)).toEqual({ start: 4, query: "login form" });
        // A heading, a colour, an anchor, a hand-typed number already followed by words.
        expect(githubQuery("# Heading", 9)).toBeNull();
        expect(githubQuery("## Two", 3)).toBeNull();
        expect(githubQuery("color:#fff", 10)).toBeNull();
        expect(githubQuery("page#top", 8)).toBeNull();
        expect(githubQuery("fix #142 and more", 17)).toBeNull();
        // A token already picked opens nothing.
        expect(githubQuery("[#142 Fix] ", 11)).toBeNull();
    });

    it("leaves the caret to the list of the sign nearer it", () => {
        expect(signAt("#login @fi", 10)).toBe("@");
        expect(signAt("@Fix #12", 8)).toBe("#");
        expect(signAt("plain words", 11)).toBeNull();
        expect(signAt("# Heading", 9)).toBeNull();
    });
});

describe("matching", () => {
    const items = [
        pull(142, "Fix the login form"),
        issue(12, "Login loops"),
        pull(140, "Dark mode", { head: "dark-theme", state: "merged" }),
        issue(1420, "Old"),
    ];

    it("offers the first few with nothing typed, in the order given", () => {
        expect(githubMatches(items, "").map((item) => item.number)).toEqual([142, 12, 140, 1420]);
        const many = Array.from({ length: 20 }, (_, at) => issue(at + 1, "x"));
        expect(githubMatches(many, "")).toHaveLength(GITHUB_LIMIT);
    });

    it("narrows by the start of the number, or by every word typed", () => {
        expect(githubMatches(items, "14").map((item) => item.number)).toEqual([142, 140, 1420]);
        expect(githubMatches(items, "142").map((item) => item.number)).toEqual([142, 1420]);
        expect(githubMatches(items, "login").map((item) => item.number)).toEqual([142, 12]);
        expect(githubMatches(items, "LOGIN issue").map((item) => item.number)).toEqual([12]);
        expect(githubMatches(items, "theme").map((item) => item.number)).toEqual([140]);
        expect(githubMatches(items, "pr dark").map((item) => item.number)).toEqual([140]);
    });

    it("asks GitHub only for what was typed and is not among the latest", () => {
        expect(asksGitHub("", [])).toBe(false);
        expect(asksGitHub("999", [])).toBe(true);
        expect(asksGitHub("login", [pull(142, "Fix the login form")])).toBe(false);
    });
});

describe("the token", () => {
    it("names the number with the title, cut where it is long", () => {
        expect(githubToken(142, "Fix the [login]\nform")).toBe("[#142 Fix the [login) form]");
        expect(githubToken(7, "  ")).toBe("[#7]");
    });

    it("takes the place of what was typed, with a space after it", () => {
        const placed = withGitHub("see #log now", 4, 8, named(pull(142, "Fix the login form")));
        expect(placed).toEqual({ value: "see [#142 Fix the login form] now", caret: 30 });
    });

    it("is read for its number, and goes whole on one key", () => {
        const text = "see [#142 Fix] and [@4 Here] and [#12] and [#142 Again] and #9";
        expect(githubIn(text)).toEqual([142, 12]);
        expect(githubIn("[Pasted text #3 +40 lines] [Quote #2: x]")).toEqual([]);
        // A Markdown link's words are no token: it names its own address.
        expect(githubIn("Fixed in [#123](https://example.com/pull/123), see [#7]")).toEqual([7]);
        expect(githubSpans("[#123](https://example.com) [#7]")).toEqual([{ start: 28, end: 32 }]);
    });

    it("asks nothing of a session with no repository", () => {
        expect(githubOf("see [#142 Fix]", "storefront")).toEqual([142]);
        expect(githubOf("see [#142 Fix]", undefined)).toEqual([]);
        expect(tokenToRemove("Backspace", "see [#142 Fix]", 14, 14)).toEqual({ start: 4, end: 14 });
        expect(tokenToRemove("Delete", "[#12] x", 0, 0)).toEqual({ start: 0, end: 5 });
    });

    it("is named by its title's words, or its number where it shows none", () => {
        expect(mentionsAsWords("fix [#142 Fix the login form] and [#12]")).toBe(
            "fix Fix the login form and #12",
        );
    });

    it("says its kind and state", () => {
        expect(githubSaid(pull(142, "x", { state: "draft" }))).toBe("Pull request #142, draft");
        expect(githubSaid(issue(12, "x"))).toBe("Issue #12, open");
    });
});

describe("the list under a message", () => {
    it("is read back as the core writes it", () => {
        const { text, github } = splitGitHub(LISTED);
        expect(text).toBe("fix what [#142 Fix the login form] left");
        expect(github).toEqual([
            {
                kind: "pull",
                number: 142,
                title: 'Fix the "login" form',
                state: "merged",
                url: "https://github.com/o/r/pull/142",
                head: "fix-login",
                base: "main",
            },
            {
                kind: "issue",
                number: 12,
                title: "Login loops",
                state: "closed",
                url: "https://github.com/o/r/issues/12",
                head: null,
                base: null,
            },
        ]);
    });

    it("is written as the core writes it, for the sample data", () => {
        const items = [named(pull(142, "Fix the login form")), named(issue(12, "  "))];
        const written = withGitHubList(" see ", items);
        expect(written).toBe(
            [
                "see",
                "",
                MENTIONED_ON_GITHUB,
                '- pull request #142 ("Fix the login form", open, fix-login into main): https://github.com/o/r/pull/142',
                '- issue #12 ("untitled", open): https://github.com/o/r/issues/12',
            ].join("\n"),
        );
        expect(splitGitHub(written).github).toEqual(
            items.map((item, at) => (at === 1 ? { ...item, title: "untitled" } : item)),
        );
        expect(withGitHubList(" words ", [])).toBe("words");
    });

    it("is only one that runs to the end, set apart by an empty line", () => {
        const typed = `${MENTIONED_ON_GITHUB}\n- issue #12 ("x", open): u\nand more`;
        expect(splitGitHub(typed).github).toEqual([]);
        const joined = `words\n${MENTIONED_ON_GITHUB}\n- issue #12 ("x", open): u`;
        expect(splitGitHub(joined)).toEqual({ text: joined, github: [] });
        expect(splitGitHub("plain").github).toEqual([]);
    });

    it("comes after the sessions and before the files, and each comes off in turn", () => {
        const sessions = withMentions("look [@9 Fix login] and [#12 Login loops]", [
            { id: 9, title: "Fix login" },
        ]);
        const message = `${withGitHubList(sessions, [named(issue(12, "Login loops"))])}\n\nAttached files:\n- /tmp/a.txt`;
        const sent = splitSent(message);
        expect(sent.text).toBe("look [@9 Fix login] and [#12 Login loops]");
        expect(sent.files).toEqual(["/tmp/a.txt"]);
        expect(sent.sessions).toEqual([{ id: 9, title: "Fix login" }]);
        expect(sent.github.map((item) => item.number)).toEqual([12]);
        expect(splitMentioned("just words")).toEqual({
            text: "just words",
            sessions: [],
            github: [],
        });
    });
});

describe("reading a message back", () => {
    it("brings each back as a token, at the end where the words named it nowhere", () => {
        expect(typedAgain(LISTED)).toBe(
            "fix what [#142 Fix the login form] left [#12 Login loops]",
        );
        const both = withGitHubList(withMentions("", [{ id: 3, title: "Three" }]), [
            named(issue(12, "Login loops")),
        ]);
        expect(typedAgain(both)).toBe("[@3 Three] [#12 Login loops]");
        expect(typedAgain("plain")).toBe("plain");
    });

    it("recalls a sent message with its tokens", () => {
        const events = [
            { type: "user_message" as const, text: LISTED, remote: false, at_reset: false },
        ];
        expect(sentMessages(events)[0]?.text).toBe(
            "fix what [#142 Fix the login form] left [#12 Login loops]",
        );
    });

    it("draws a token the list names as what it names, and leaves the rest words", () => {
        const { text, github } = splitGitHub(LISTED);
        const pieces = mentionedPieces(`${text} [#77 Not listed] [@4 Nor this]`, [], github);
        expect(pieces[0]).toBe("fix what ");
        expect(pieces[1]).toMatchObject({ github: { number: 142, shown: "Fix the login form" } });
        expect(pieces[2]).toBe(" left [#77 Not listed] [@4 Nor this]");
        const session = mentionedPieces("[@4 Here]", [{ id: 4, title: "Old title" }], []);
        expect(session).toEqual([{ session: { id: 4, title: "Here" } }]);
    });

    it("lists apart what the words name nowhere, as the command line sends it", () => {
        const { text, github } = splitGitHub(LISTED);
        expect(githubApart(text, github).map((item) => item.number)).toEqual([12]);
    });
});

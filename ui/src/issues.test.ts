import { describe, expect, it } from "vitest";
import type { Issue, Project, Session, Task } from "./api/types";
import { CHATS } from "./chats";
import {
    ageOf,
    FIRST_FILTER,
    issueLine,
    issueLink,
    issuePrompt,
    issueSaid,
    issuesCloseBeside,
    issuesRepository,
    shownIssues,
    withIssuePrompt,
} from "./issues";

const DAY = 86_400;
const NOW = Date.parse("2026-11-25T12:00:00Z") / 1000;

const issue = (number: number, title: string, more: Partial<Issue> = {}): Issue => ({
    number,
    title,
    state: "open",
    url: `https://github.com/o/r/issues/${number}`,
    labels: [],
    assignees: [],
    author: "sam",
    updated_at: "2026-11-24T12:00:00Z",
    ...more,
});

describe("shownIssues", () => {
    const issues = [
        issue(428, "Login redirect loops", { labels: ["bug"] }),
        issue(425, "Invoice export drops VAT", { author: "you" }),
        issue(410, "Highlight search words", { labels: ["good first issue"] }),
    ];
    const numbers = (shown: Issue[]) => shown.map((one) => one.number);

    it("shows every issue for no words", () => {
        expect(numbers(shownIssues(issues, "  "))).toEqual([428, 425, 410]);
    });

    it("keeps those holding every word, in their title, labels, author or number", () => {
        expect(numbers(shownIssues(issues, "LOGIN loops"))).toEqual([428]);
        expect(numbers(shownIssues(issues, "bug"))).toEqual([428]);
        expect(numbers(shownIssues(issues, "you"))).toEqual([425]);
        expect(numbers(shownIssues(issues, "#410"))).toEqual([410]);
        expect(numbers(shownIssues(issues, "login vat"))).toEqual([]);
    });

    it("leaves GitHub's own syntax to GitHub", () => {
        expect(numbers(shownIssues(issues, "label:bug"))).toEqual([428, 425, 410]);
        expect(numbers(shownIssues(issues, "is:open invoice"))).toEqual([425]);
    });
});

describe("issuePrompt", () => {
    it("names the issue, where it is, and how to read it, without what it says", () => {
        const prompt = issuePrompt(issue(428, "Login redirect loops"));
        expect(prompt).toBe(
            [
                "Login redirect loops (issue #428)",
                "https://github.com/o/r/issues/428",
                "",
                "Work on this issue of the repository. Read it first, with its comments: `gh issue view 428 --comments`.",
            ].join("\n"),
        );
    });

    it("starts with the title, which a task named without a model is named after", () => {
        const first = issuePrompt(issue(428, "Login redirect loops")).split("\n")[0];
        expect(first).toBe("Login redirect loops (issue #428)");
    });
});

describe("withIssuePrompt", () => {
    const before = issueLink(issue(1, "First"));
    const prompt = issuePrompt(issue(2, "Second"));

    it("fills an empty box", () => {
        expect(withIssuePrompt("", null, prompt)).toBe(prompt);
        expect(withIssuePrompt("  \n", before, prompt)).toBe(prompt);
    });

    it("replaces the prompt of the issue it was opened for before, untouched", () => {
        expect(withIssuePrompt(`${before.prompt}\n`, before, prompt)).toBe(prompt);
    });

    it("keeps what the user typed, under the prompt", () => {
        expect(withIssuePrompt("also check Safari", null, prompt)).toBe(
            `${prompt}\n\nalso check Safari`,
        );
        const changed = `${before.prompt}\nand Safari`;
        expect(withIssuePrompt(changed, before, prompt)).toBe(`${prompt}\n\n${changed}`);
    });
});

describe("issuesRepository", () => {
    const session = { id: 7, task_id: 3 } as Session;
    const task = { id: 3, name: "fix", archived: false, sessions: [session] } as Task;
    const repository = (name: string, tasks: Task[] = []) =>
        ({ name, chats: false, tasks }) as unknown as Project;
    const chats = { name: CHATS, chats: true, tasks: [] } as unknown as Project;
    const projects = [chats, repository("storefront"), repository("fsum", [task])];

    it("is the repository of the session on screen", () => {
        expect(issuesRepository(projects, 7, "storefront")).toBe("fsum");
    });

    it("is the new-task view's repository with no session on screen", () => {
        expect(issuesRepository(projects, null, "fsum")).toBe("fsum");
    });

    it("is the first repository otherwise, and never the chats", () => {
        expect(issuesRepository(projects, null, CHATS)).toBe("storefront");
        expect(issuesRepository(projects, null, null)).toBe("storefront");
        expect(issuesRepository([chats], null, CHATS)).toBeNull();
    });
});

describe("issueLine", () => {
    it("says the number, state, author, labels and age", () => {
        const closed = issue(9, "Old", {
            state: "closed",
            labels: ["bug", "ui"],
            updated_at: new Date((NOW - 3 * DAY) * 1000).toISOString(),
        });
        expect(issueLine(closed, NOW)).toBe("#9 · closed · by sam · bug, ui · changed 3 days ago");
    });

    it("leaves out what it does not know", () => {
        const bare = issue(9, "Bare", { author: "", updated_at: "" });
        expect(issueLine(bare, NOW)).toBe("#9");
        expect(ageOf("not a time", NOW)).toBeNull();
    });
});

describe("issueSaid", () => {
    it("says the number until the issue is read, then its state and title", () => {
        expect(issueSaid(12, null)).toBe("Issue #12");
        expect(issueSaid(12, issue(12, "Login loops"))).toBe("Issue #12, open: Login loops");
        expect(issueSaid(12, issue(12, "Login loops", { state: "closed" }))).toBe(
            "Issue #12, closed: Login loops",
        );
    });
});

describe("issuesCloseBeside", () => {
    it("closes on a click beside it only while nothing is typed or chosen", () => {
        expect(issuesCloseBeside(FIRST_FILTER, "")).toBe(true);
        expect(issuesCloseBeside(FIRST_FILTER, "login")).toBe(false);
        expect(issuesCloseBeside({ ...FIRST_FILTER, who: "assigned" }, "")).toBe(false);
        expect(issuesCloseBeside({ ...FIRST_FILTER, state: "closed" }, "")).toBe(false);
    });
});

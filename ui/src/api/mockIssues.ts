// The sample repositories' GitHub issues, as the core reads them through `gh` (`issues.rs`):
// storefront has a handful, one of them its task `shop-142`'s, bonsai-lint two, and fsum is not
// on GitHub, so its list is empty and says why. One issue's text holds HTML and an image, to
// check that issue text is drawn as text.

import { shownIssues } from "../issues";
import type { Api } from "./client";
import type { Issue, IssueDetail, IssueFilter, Project } from "./types";

/** The user's login on GitHub, in the sample. */
const YOU = "you";

/** How long the sample's `gh` takes to answer, in milliseconds. */
const ANSWERS_IN = 350;

const HOUR = 3_600_000;

/** When something changed, `hours` ago, as GitHub writes it. */
function hoursAgo(hours: number): string {
    return new Date(Date.now() - hours * HOUR).toISOString().replace(/\.\d+Z$/, "Z");
}

interface SampleIssue extends IssueDetail {
    /** The logins it mentions, for the filter of issues that mention the user. */
    mentions: string[];
}

function sample(
    project: string,
    issue: Omit<Issue, "url" | "updated_at"> & { hours: number },
    body: string,
    extra: Partial<SampleIssue> = {},
): SampleIssue {
    const { hours, ...rest } = issue;
    return {
        ...rest,
        url: `https://github.com/example/${project}/issues/${issue.number}`,
        updated_at: hoursAgo(hours),
        created_at: hoursAgo(hours + 24 * 9),
        body,
        comments: [],
        mentions: [],
        ...extra,
    };
}

/** What anyone can write in an issue: HTML, a script and an image, all of it shown as text. */
const UNTRUSTED_BODY = [
    "After logging in, the browser goes back to `/login` and loops.",
    "",
    "**Steps**",
    "",
    "1. Open `/login?next=/login`",
    "2. Log in",
    "",
    '<img src="x" onerror="alert(1)"> <script>alert("not run")</script>',
    "",
    "![screenshot of the loop](https://example.com/loop.png)",
    "",
    "```html-view",
    "<h1>Never drawn as a view</h1>",
    "```",
].join("\n");

const ISSUES: Record<string, SampleIssue[]> = {
    storefront: [
        sample(
            "storefront",
            {
                number: 431,
                title: "Checkout: accept discount codes in any case",
                state: "open",
                labels: ["enhancement"],
                assignees: [],
                author: "mira",
                hours: 2,
            },
            "`SUMMER10` works and `summer10` does not.",
        ),
        sample(
            "storefront",
            {
                number: 428,
                title: "Login redirect loops when `next` points at the login page",
                state: "open",
                labels: ["bug"],
                assignees: [YOU],
                author: "sam",
                hours: 5,
            },
            UNTRUSTED_BODY,
            {
                comments: [
                    {
                        author: "mira",
                        body: "Same on Safari. @you, could you take this one?",
                        created_at: hoursAgo(30),
                        hidden: null,
                    },
                    {
                        author: "bot-of-spam",
                        body: "",
                        created_at: hoursAgo(20),
                        hidden: "spam",
                    },
                    {
                        author: YOU,
                        body: "On it.",
                        created_at: hoursAgo(6),
                        hidden: null,
                    },
                ],
                mentions: [YOU],
            },
        ),
        sample(
            "storefront",
            {
                number: 425,
                title: "Invoice CSV export drops the VAT column",
                state: "open",
                labels: ["bug", "invoices"],
                assignees: [],
                author: YOU,
                hours: 26,
            },
            "Exports since 2.3 have no `vat` column. Accounting needs it by Friday.",
        ),
        sample(
            "storefront",
            {
                number: 410,
                title: "Search results: highlight the words searched for",
                state: "open",
                labels: ["good first issue"],
                assignees: [],
                author: "sam",
                hours: 50,
            },
            "Like GitHub does in its own search. cc @you",
            { mentions: [YOU] },
        ),
        sample(
            "storefront",
            {
                number: 402,
                title: "Move to the new payments API before March",
                state: "open",
                labels: ["chore"],
                assignees: ["mira"],
                author: "mira",
                hours: 120,
            },
            "The old one is switched off on 1 March.",
        ),
        sample(
            "storefront",
            {
                number: 142,
                title: "Shop page shows prices in the wrong currency",
                state: "open",
                labels: ["bug"],
                assignees: [YOU],
                author: "sam",
                hours: 200,
            },
            "Switching the country to Germany keeps the prices in pounds.",
        ),
        sample(
            "storefront",
            {
                number: 419,
                title: "Add a dark mode that follows the system",
                state: "closed",
                labels: ["enhancement"],
                assignees: [YOU],
                author: "mira",
                hours: 48,
            },
            "Follow `prefers-color-scheme`.",
        ),
        sample(
            "storefront",
            {
                number: 398,
                title: "Product images load slowly on phones",
                state: "closed",
                labels: [],
                assignees: [],
                author: "sam",
                hours: 400,
            },
            "Fixed by serving them in WebP.",
        ),
    ],
    "bonsai-lint": [
        sample(
            "bonsai-lint",
            {
                number: 88,
                title: "Support Rust",
                state: "open",
                labels: ["language"],
                assignees: [YOU],
                author: YOU,
                hours: 10,
            },
            "Functions, closures and match arms, counted as the other languages are.",
        ),
        sample(
            "bonsai-lint",
            {
                number: 91,
                title: "A threshold per folder",
                state: "open",
                labels: [],
                assignees: [],
                author: "kim",
                hours: 70,
            },
            "Tests may nest deeper than the code they test.",
        ),
    ],
};

/** What `gh` says in a repository that is not on GitHub, as fsum is in the sample. */
const NOT_ON_GITHUB =
    "none of the git remotes configured for this repository point to a known GitHub host";

/** An issue as a list shows it: without what it says, its comments or whom it mentions. */
function listed(issue: SampleIssue): Issue {
    const { body: _body, comments: _comments, created_at: _created, mentions: _m, ...rest } = issue;
    return rest;
}

/** Whether `issue` is one of those `filter` asks for, as GitHub would pick them. */
function picked(issue: SampleIssue, filter: IssueFilter): boolean {
    const whose = {
        anyone: true,
        assigned: issue.assignees.includes(YOU),
        authored: issue.author === YOU,
        mentioned: issue.mentions.includes(YOU),
    }[filter.who];
    return whose && issue.state === filter.state;
}

function wait(): Promise<void> {
    return new Promise((done) => setTimeout(done, ANSWERS_IN));
}

function notFound(number: number): Error {
    return new Error(
        `GraphQL: Could not resolve to an issue or pull request with the number of ${number}. (repository.issue)`,
    );
}

export function issuesApi(
    projects: () => Project[],
): Pick<Api, "projectIssues" | "projectIssue" | "taskIssue"> {
    const of = (project: string) => ISSUES[project];
    const one = (project: string, number: number) => {
        const found = of(project)?.find((issue) => issue.number === number);
        if (!found) {
            throw notFound(number);
        }
        return found;
    };
    return {
        projectIssues: async (project, filter) => {
            await wait();
            const issues = of(project);
            if (!issues) {
                return { issues: [], unavailable: NOT_ON_GITHUB };
            }
            const chosen = issues.filter((issue) => picked(issue, filter)).map(listed);
            const found = shownIssues(chosen, filter.search).slice(0, filter.limit);
            return { issues: found, unavailable: null };
        },
        projectIssue: async (project, number) => {
            await wait();
            const { mentions: _mentions, ...detail } = one(project, number);
            return structuredClone(detail);
        },
        taskIssue: async (project, name) => {
            await wait();
            const tasks = projects().find((candidate) => candidate.name === project)?.tasks ?? [];
            const number = tasks.find((task) => task.name === name && !task.archived)?.issue;
            return number == null ? null : listed(one(project, number));
        },
    };
}

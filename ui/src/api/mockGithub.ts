// The sample repositories' pull requests and issues as `#` in the message box offers them, as
// the core reads them through `gh` (`github_mentions.rs`): storefront's and bonsai-lint's pull
// requests beside their issues (`mockIssues.ts`), and none for fsum, which is not on GitHub, with
// why. GitHub counts pull requests and issues alike, so an issue numbered as a pull request here
// is left out.

import { CHATS } from "../chats";
import { type GitHubMentioned, githubMatches } from "../githubMentions";
import type { Api } from "./client";
import { sampleIssues } from "./mockIssues";
import type { GitHubItem } from "./types";

/** How long the sample's `gh` takes to answer, in milliseconds. */
const ANSWERS_IN = 300;

const HOUR = 3_600_000;

function hoursAgo(hours: number): string {
    return new Date(Date.now() - hours * HOUR).toISOString().replace(/\.\d+Z$/, "Z");
}

function pull(
    project: string,
    number: number,
    title: string,
    state: GitHubItem["state"],
    head: string,
    hours: number,
): GitHubItem {
    const url = `https://github.com/example/${project}/pull/${number}`;
    return {
        kind: "pull",
        number,
        title,
        state,
        url,
        head,
        base: "main",
        updated_at: hoursAgo(hours),
    };
}

/** Each sample repository's pull requests: its tasks' own, and others'. */
const PULLS: Record<string, GitHubItem[]> = {
    storefront: [
        pull(
            "storefront",
            412,
            "Fix the redirect loop after login",
            "open",
            "fix-login-redirect",
            1,
        ),
        pull("storefront", 415, "Show the shop's opening hours", "draft", "shop-142", 3),
        pull("storefront", 420, "Cache product images on the CDN", "open", "image-cache", 7),
        pull(
            "storefront",
            398,
            "Add a dark mode that follows the system",
            "merged",
            "dark-mode",
            30,
        ),
        pull("storefront", 409, "Bump the payments SDK", "closed", "payments-sdk", 70),
    ],
    "bonsai-lint": [
        pull("bonsai-lint", 87, "Lint Rust files", "merged", "rust-support", 20),
        pull("bonsai-lint", 93, "A threshold per folder", "open", "folder-threshold", 2),
    ],
};

/** What `gh` says in a repository that is not on GitHub, as fsum is in the sample. */
const NOT_ON_GITHUB =
    "none of the git remotes configured for this repository point to a known GitHub host";

/** A sample repository's pull requests and issues, the latest changed first; `undefined` off GitHub. */
function itemsOf(project: string): GitHubItem[] | undefined {
    if (project === CHATS) {
        throw new Error("a chat has no repository: start a task in a repository for that");
    }
    const pulls = PULLS[project];
    const issues = sampleIssues(project);
    if (!(pulls || issues)) {
        return undefined;
    }
    const numbers = new Set((pulls ?? []).map((one) => one.number));
    const asItems = (issues ?? [])
        .filter((issue) => !numbers.has(issue.number))
        .map(({ number, title, state, url, updated_at }) => ({
            kind: "issue" as const,
            number,
            title,
            state,
            url,
            head: null,
            base: null,
            updated_at,
        }));
    const all = [...(pulls ?? []), ...asItems];
    return all.sort((a, b) => b.updated_at.localeCompare(a.updated_at) || b.number - a.number);
}

function notFound(number: number): string {
    return `GraphQL: Could not resolve to an issue or pull request with the number of ${number}.`;
}

/** Number `number` of a sample repository, or the core's refusal. */
function one(project: string, number: number): GitHubItem {
    const found = itemsOf(project)?.find((item) => item.number === number);
    if (!found) {
        throw new Error(`#${number} cannot be mentioned: ${notFound(number)}`);
    }
    return found;
}

/** The pull requests and issues `numbers` of a sample repository, as a message lists them. */
export function githubFor(project: string, numbers: number[]): GitHubMentioned[] {
    return [...new Set(numbers)].map((number) => {
        const { updated_at: _changed, ...item } = one(project, number);
        return item;
    });
}

function wait(): Promise<void> {
    return new Promise((done) => setTimeout(done, ANSWERS_IN));
}

export function githubApi(): Pick<Api, "projectGithubItems" | "projectGithubRead"> {
    return {
        projectGithubItems: async (project, query) => {
            await wait();
            const items = itemsOf(project);
            if (!items) {
                return { items: [], unavailable: NOT_ON_GITHUB };
            }
            const typed = query.trim();
            if (/^\d+$/.test(typed)) {
                const found = items.filter((item) => item.number === Number(typed));
                const unavailable = found.length === 0 ? notFound(Number(typed)) : null;
                return { items: found, unavailable };
            }
            // GitHub's search, as near as the sample comes: the words in the title or branch.
            const found = typed === "" ? items : githubMatches(items, typed);
            return { items: structuredClone(found), unavailable: null };
        },
        projectGithubRead: async (project, numbers) => {
            await wait();
            return numbers.map((number) => structuredClone(one(project, number)));
        },
    };
}

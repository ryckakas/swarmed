// A repository's GitHub issues in the window: the filters, what the list shows while GitHub is
// asked again, the prompt a task started on an issue begins with, and how a task's issue is
// told. Pure, so it can be tested; the Issues view and the store only call these.

import type { Issue, IssueFilter, IssueState, IssueWho, Project } from "./api/types";
import { age } from "./cleanup";
import { findSession, findSessionHome } from "./tree";

/** What the Issues view starts on: everyone's open issues, the latest changed first. */
export const FIRST_FILTER: IssueFilter = { who: "anyone", state: "open", search: "", limit: 50 };

/**
 * Whether a click beside the Issues view closes it: only while nothing is typed in its search
 * box and its filters are as it opened, so that a slip of the pointer loses nothing chosen.
 */
export function issuesCloseBeside(filter: IssueFilter, typed: string): boolean {
    const chosen = filter.who !== FIRST_FILTER.who || filter.state !== FIRST_FILTER.state;
    return typed.trim() === "" && !chosen;
}

/** Whose issues, as the menu offers them. */
export const WHO_CHOICES: [IssueWho, string][] = [
    ["anyone", "Anyone's"],
    ["assigned", "Assigned to me"],
    ["authored", "Opened by me"],
    ["mentioned", "Mentioning me"],
];

/** Open ones or closed ones, as the menu offers them. */
export const STATE_CHOICES: [IssueState, string][] = [
    ["open", "Open"],
    ["closed", "Closed"],
];

/** What an issue is found by among those listed: its number, title, labels and author. */
function foundBy(issue: Issue): string {
    return [`#${issue.number}`, issue.title, ...issue.labels, issue.author].join(" ").toLowerCase();
}

/**
 * The issues of a list that hold every word typed, for the list to show at once while GitHub
 * is asked again with the words. A word in GitHub's own syntax, such as `label:bug`, is left
 * to GitHub: it holds back nothing here.
 */
export function shownIssues(issues: Issue[], typed: string): Issue[] {
    const words = typed
        .toLowerCase()
        .split(/\s+/)
        .filter((word) => word !== "" && !word.includes(":"));
    if (words.length === 0) {
        return issues;
    }
    return issues.filter((issue) => {
        const text = foundBy(issue);
        return words.every((word) => text.includes(word.replace(/^#/, "")));
    });
}

/**
 * The prompt a task started on an issue begins with: the issue's title and number, its address,
 * and a line asking the agent to read it, comments included, with `gh`. The title comes first,
 * since a session is titled after its first line, and a task named without a model after its
 * first words. What the issue says is not pasted in: the agent reads it as it reads anything
 * else, and the user reads this before sending it.
 */
export function issuePrompt(issue: Pick<Issue, "number" | "title" | "url">): string {
    return [
        `${issue.title} (issue #${issue.number})`,
        issue.url,
        "",
        `Work on this issue of the repository. Read it first, with its comments: \`gh issue view ${issue.number} --comments\`.`,
    ].join("\n");
}

/** The repository whose issues the Issues view lists, and the issue it opens on, if any. */
export interface IssuesTarget {
    project: string;
    issue: number | null;
}

/** The issue a new task is started for, as the new-task view of a repository keeps it. */
export interface IssueLink {
    number: number;
    title: string;
    /** The prompt that was put into the box for it, as it was put there. */
    prompt: string;
}

/**
 * What the new-task box holds once an issue's prompt is put in. An empty box, or one holding
 * only the prompt of the issue it was opened for before, gets the new prompt; anything the user
 * typed stays, under it.
 */
export function withIssuePrompt(typed: string, before: IssueLink | null, prompt: string): string {
    const kept = typed.trim();
    if (kept === "" || kept === before?.prompt.trim()) {
        return prompt;
    }
    return `${prompt}\n\n${kept}`;
}

/** The link a new task keeps to `issue`, with the prompt put in for it. */
export function issueLink(issue: Pick<Issue, "number" | "title" | "url">): IssueLink {
    return { number: issue.number, title: issue.title, prompt: issuePrompt(issue) };
}

/**
 * The repository whose issues the keyboard opens: that of the session on screen, else that of
 * the new-task view, else the first one there is. The chats have none.
 */
export function issuesRepository(
    projects: Project[],
    selectedSessionId: number | null,
    draftProject: string | null,
): string | null {
    const session = findSession(projects, selectedSessionId);
    const home = session && findSessionHome(projects, session);
    const repositories = projects.filter((project) => !project.chats);
    const named = (name: string | null | undefined) =>
        repositories.find((project) => project.name === name)?.name;
    const first = repositories[0]?.name;
    return named(home?.project.name) ?? named(draftProject) ?? first ?? null;
}

/** How long ago GitHub's moment `at` was, as people say it; `null` for one it did not give. */
export function ageOf(at: string, now: number): string | null {
    const moment = Date.parse(at);
    return Number.isNaN(moment) ? null : age(moment / 1000, now);
}

/** An issue's state, labels, author and age, on one line, as the list shows it under its title. */
export function issueLine(issue: Issue, now: number): string {
    const changed = ageOf(issue.updated_at, now);
    const when = changed === null ? null : `changed ${changed}`;
    const by = issue.author === "" ? null : `by ${issue.author}`;
    const labels = issue.labels.length > 0 ? issue.labels.join(", ") : null;
    return [`#${issue.number}`, issue.state === "closed" ? "closed" : null, by, labels, when]
        .filter((part) => part !== null)
        .join(" · ");
}

/** What the task panel says of the issue a task is for: its number, state and title. */
export function issueSaid(number: number, issue: Issue | null): string {
    if (issue === null) {
        return `Issue #${number}`;
    }
    const state = issue.state === "closed" ? "closed" : "open";
    return `Issue #${number}, ${state}: ${issue.title}`;
}

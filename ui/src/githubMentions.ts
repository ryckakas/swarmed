// Mentioning a pull request or an issue in a message with `#`, for the agent to read with `gh`.
// Pure, so it can be tested. Built on the pattern of `@` for sessions (`mentions.ts`): a menu
// opened by the sign, a token, a fixed list under the message, and reading it back.
//
// A pull request or issue the user picks stands in the box as one token,
// `[#142 Fix the login form]`, which stays in the message where it was typed. The core lists
// each one the tokens name under the message, with its title, state, branches and address, and
// that list is read back here, to show each as a chip coloured by its state and to bring a
// sent message back into a box with its tokens. A `#` with no pick after it, such as a heading
// or a number typed by hand, stays text and is not looked up.

import type { GitHubItem, GitHubKind, GitHubState } from "./api/types";
import {
    type Mentioned,
    mentionQuery,
    mentionsAgain,
    placeToken,
    splitMentions,
    tokenLabel,
} from "./mentions";

/** How many the list shows at once. */
export const GITHUB_LIMIT = 8;

/** A pull request's or issue's token as it stands in the box and in the message. */
const TOKEN = /\[#(\d+)(?: ([^\]\n]*))?\]/g;

/** Either kind of token, for taking a message apart where they stand. */
const ANY_TOKEN = /\[([@#])(\d+)(?: ([^\]\n]*))?\]/g;

/** The line that opens the list under a message, as the core writes it. */
export const MENTIONED_ON_GITHUB = "Pull requests and issues mentioned, to read with gh:";

/**
 * One line of that list:
 * `- pull request #142 ("Fix the login form", open, fix-login into main): <address>`.
 */
const LISTED =
    /^- (pull request|issue) #(\d+) \("(.*)", (open|draft|merged|closed)(?:, (\S+) into (\S+))?\): (https:\/\/\S+)$/;

/** A pull request or issue as a sent message names it. */
export type GitHubMentioned = Omit<GitHubItem, "updated_at">;

/** The token that stands for number `number` called `title`. */
export function githubToken(number: number, title: string): string {
    const label = tokenLabel(title);
    return label === "" ? `[#${number}]` : `[#${number} ${label}]`;
}

/** The numbers the tokens in `text` name, in the order they first stand there, each once. */
export function githubIn(text: string): number[] {
    return [...new Set([...text.matchAll(TOKEN)].map((match) => Number(match[1])))];
}

/** Where each token in `text` stands, for taking one out whole. */
export function githubSpans(text: string): { start: number; end: number }[] {
    return [...text.matchAll(TOKEN)].map((match) => ({
        start: match.index,
        end: match.index + match[0].length,
    }));
}

/**
 * What has been typed of a `#` mention with the caret at `caret`, as `mentionQuery` reads one.
 * A number with a space after it was typed by hand and is done: what follows is words, not a
 * search.
 */
export function githubQuery(value: string, caret: number) {
    const typed = mentionQuery(value, caret, "#");
    return typed && /^\d+\s/.test(typed.query) ? null : typed;
}

/** What a word typed is matched against: the title, the number, the branch and the kind. */
function matchedOn(item: GitHubItem): string {
    const kind = item.kind === "pull" ? "pull request pr" : "issue";
    return `${item.title} ${item.number} ${item.head ?? ""} ${kind}`.toLowerCase();
}

/**
 * The pull requests and issues a query offers, in the order the core gives them (the latest
 * changed first): with nothing typed, the first few; digits, those whose number starts so;
 * words, those with every word in their title, number, branch or kind.
 */
export function githubMatches(items: GitHubItem[], query: string): GitHubItem[] {
    const typed = query.trim().toLowerCase();
    if (/^\d+$/.test(typed)) {
        return items.filter((item) => String(item.number).startsWith(typed)).slice(0, GITHUB_LIMIT);
    }
    const words = typed.split(/\s+/).filter(Boolean);
    const found = items.filter((item) => words.every((word) => matchedOn(item).includes(word)));
    return found.slice(0, GITHUB_LIMIT);
}

/**
 * Whether the core is to be asked for `query` itself, as the recent ones it gave hold no
 * match: a number further back, or words GitHub's search may find.
 */
export function asksGitHub(query: string, matches: GitHubItem[]): boolean {
    return query.trim() !== "" && matches.length === 0;
}

/**
 * `value` with the token for `item` in place of what was typed of it, from the `#` at `start`
 * to `caret`, and a space after it; and where the caret goes, after that space.
 */
export function withGitHub(value: string, start: number, caret: number, item: GitHubMentioned) {
    return placeToken(value, start, caret, githubToken(item.number, item.title));
}

/** How a pull request's or issue's kind and state read, for a chip's or a row's hint. */
export function githubSaid(item: Pick<GitHubItem, "kind" | "number" | "state">): string {
    const kind = item.kind === "pull" ? "Pull request" : "Issue";
    return `${kind} #${item.number}, ${item.state}`;
}

/** One line of the list under a message, as the core writes it. */
function listedLine(item: GitHubMentioned): string {
    const kind = item.kind === "pull" ? "pull request" : "issue";
    const title = item.title.replace(/\s+/g, " ").trim() || "untitled";
    const branches = item.head && item.base ? `, ${item.head} into ${item.base}` : "";
    return `- ${kind} #${item.number} ("${title}", ${item.state}${branches}): ${item.url}`;
}

/**
 * `text` with `items` listed under it as the core lists them. For the sample data, which has no
 * core to write it.
 */
export function withGitHubList(text: string, items: GitHubMentioned[]): string {
    const trimmed = text.trim();
    if (items.length === 0) {
        return trimmed;
    }
    const list = [MENTIONED_ON_GITHUB, ...items.map(listedLine)].join("\n");
    return trimmed === "" ? list : `${trimmed}\n\n${list}`;
}

/** The pull request or issue a line of the list names, if it is one. */
function listedIn(line: string): GitHubMentioned | null {
    const match = LISTED.exec(line);
    if (!match) {
        return null;
    }
    const [, kind, number, title, state, head, base, url] = match;
    return {
        kind: (kind === "issue" ? "issue" : "pull") as GitHubKind,
        number: Number(number),
        title: title ?? "",
        state: (state ?? "open") as GitHubState,
        url: url ?? "",
        head: head ?? null,
        base: base ?? null,
    };
}

/**
 * A message taken apart again: what the user wrote, and the pull requests and issues listed
 * under it. Only a list that runs to the end of the message, set apart by an empty line, is
 * one. Attached files come after it, so they are taken off first (`splitFiles`); the sessions
 * it mentions come before it, and stay in the text.
 */
export function splitGitHub(message: string): { text: string; github: GitHubMentioned[] } {
    const lines = message.split("\n");
    const at = lines.lastIndexOf(MENTIONED_ON_GITHUB);
    const listed = at === -1 ? [] : lines.slice(at + 1).map(listedIn);
    const whole = listed.length > 0 && listed.every((item) => item !== null);
    if (!whole || (at > 0 && lines[at - 1] !== "")) {
        return { text: message, github: [] };
    }
    const text = lines.slice(0, Math.max(at - 1, 0)).join("\n");
    return { text, github: listed.filter((item) => item !== null) };
}

/**
 * A message taken apart into what the user wrote, the sessions it mentions and its pull
 * requests and issues: the lists under it come off from the end, the last first.
 */
export function splitMentioned(message: string): {
    text: string;
    sessions: Mentioned[];
    github: GitHubMentioned[];
} {
    const { text, github } = splitGitHub(message);
    return { ...splitMentions(text), github };
}

/**
 * A sent message's words as they can be typed again: the lists under it gone, and each session,
 * pull request and issue it names standing as a token, where it had none in the words (named on
 * the command line, say) at the end. For bringing a message back into a box.
 */
export function typedAgain(message: string): string {
    const { text, github } = splitGitHub(message);
    const words = mentionsAgain(text);
    const named = new Set(githubIn(words));
    const added = github
        .filter((item) => !named.has(item.number))
        .map((item) => githubToken(item.number, item.title));
    if (added.length === 0) {
        return words;
    }
    return words === "" ? added.join(" ") : `${words} ${added.join(" ")}`;
}

/** A piece of what the user wrote: words, a session it names, or a pull request or issue. */
export type MentionedPiece =
    | string
    | { session: Mentioned }
    | { github: GitHubMentioned & { shown: string } };

/**
 * `text` in pieces: the tokens of what is listed under the message as what they name, the rest
 * as it was typed. A token the list does not name was not checked by the core, and stays words.
 */
export function mentionedPieces(
    text: string,
    sessions: Mentioned[],
    github: GitHubMentioned[],
): MentionedPiece[] {
    const pieces: MentionedPiece[] = [];
    let from = 0;
    for (const match of text.matchAll(ANY_TOKEN)) {
        const piece = pieceOf(match, sessions, github);
        if (piece === null) {
            continue;
        }
        if (match.index > from) {
            pieces.push(text.slice(from, match.index));
        }
        pieces.push(piece);
        from = match.index + match[0].length;
    }
    if (from < text.length) {
        pieces.push(text.slice(from));
    }
    return pieces;
}

/** What a token names, if the list under the message names it too. */
function pieceOf(
    match: RegExpExecArray | RegExpMatchArray,
    sessions: Mentioned[],
    github: GitHubMentioned[],
): MentionedPiece | null {
    const [, sign, number, shown] = match;
    if (sign === "@") {
        const session = sessions.find((one) => one.id === Number(number));
        return session ? { session: { id: session.id, title: shown ?? session.title } } : null;
    }
    const item = github.find((one) => one.number === Number(number));
    return item ? { github: { ...item, shown: shown ?? item.title } } : null;
}

/** The pull requests and issues listed under a message that no token in its words names. */
export function githubApart(text: string, listed: GitHubMentioned[]): GitHubMentioned[] {
    const named = new Set(githubIn(text));
    return listed.filter((item) => !named.has(item.number));
}

// Mentioning another session in a message with `@`, for the agent to read. Pure, so it can be
// tested.
//
// A session the user picks stands in the box as one token, `[@42 Fix the login form]`, which
// stays in the message where it was typed, so that the sentence around it still reads. The core
// checks each session the tokens name and lists them under the message in a fixed form, which
// is what tells the agent how to read them; that form is read back here, to show each as a chip
// in the transcript and to bring a sent message back into a box with its tokens.
//
// The pattern is meant for more than sessions: a menu opened by a sign typed at a word's start,
// a token that names one thing by its number, a list under the message, and reading it back.

import type { Reachable } from "./api/types";

/** The most of a title a token shows. */
const LABEL_CHARS = 40;

/** How many sessions the list shows at once. */
export const MENTION_LIMIT = 8;

/** The most that is read as what has been typed of a mention, before it is taken for words. */
const QUERY_CHARS = 40;

/** A session's token as it stands in the box and in the message: `[@42 Fix the login form]`. */
const TOKEN = /\[@(\d+)(?: ([^\]\n]*))?\]/g;

/**
 * The lines that open the list of sessions under a message, as the core writes them: for an
 * agent that reads them itself, and for one that is given what was said in each instead.
 */
export const MENTIONED_SESSIONS = "Sessions mentioned, to read with the read_session tool:";
export const MENTIONED_SESSIONS_SAID = "Sessions mentioned, with what was last said in each:";

/** One session of that list: `- session 42 ("Fix the login form")`, `:` after it with a digest. */
const LISTED = /^- session (\d+) \("(.*)"\):?$/;

/** A session as a sent message names it. */
export interface Mentioned {
    id: number;
    /** Its title when the message went. */
    title: string;
}

/**
 * A title as a token shows it: on one line, with nothing in it that would end the token early,
 * cut at a word with `…` after it when it is long.
 */
export function tokenLabel(title: string): string {
    const plain = title.replace(/\s+/g, " ").trim().replaceAll("]", ")");
    if (plain.length <= LABEL_CHARS) {
        return plain;
    }
    // One more than shown, so that a word that ends right at the limit is kept.
    const space = plain.slice(0, LABEL_CHARS + 1).lastIndexOf(" ");
    return `${plain.slice(0, space > 0 ? space : LABEL_CHARS).trimEnd()}…`;
}

/** The token that stands for session `id` called `title`. */
export function mentionToken(id: number, title: string): string {
    const label = tokenLabel(title);
    return label === "" ? `[@${id}]` : `[@${id} ${label}]`;
}

/** The sessions the tokens in `text` name, in the order they first stand there, each once. */
export function mentionedIn(text: string): number[] {
    const ids = [...text.matchAll(TOKEN)].map((match) => Number(match[1]));
    return [...new Set(ids)];
}

/** Where each token in `text` stands, for taking one out whole. */
export function mentionSpans(text: string): { start: number; end: number }[] {
    return [...text.matchAll(TOKEN)].map((match) => ({
        start: match.index,
        end: match.index + match[0].length,
    }));
}

/**
 * What has been typed of a mention with the caret at `caret`: from an `@` at the start of the
 * box or of a word, up to the caret, on one line. `null` when the caret is not in one: an `@`
 * inside a word, such as an address, starts none, and neither does one with a space right
 * after it. Words with spaces between them are read, since titles have them.
 */
export function mentionQuery(
    value: string,
    caret: number,
): { start: number; query: string } | null {
    const before = value.slice(0, caret);
    const start = before.lastIndexOf("@");
    if (start === -1) {
        return null;
    }
    const opens = start === 0 || /[\s(]/.test(before[start - 1] ?? "");
    const query = before.slice(start + 1);
    const plain = !(/[\n[\]]/.test(query) || query.startsWith(" "));
    return opens && plain && query.length <= QUERY_CHARS ? { start, query } : null;
}

/** What a query is matched against: the title, the task, the repository and the id. */
function matchedOn(session: Reachable): string {
    return `${session.title} ${session.task} ${session.project} ${session.id}`.toLowerCase();
}

/**
 * The sessions a query offers, in the order the core gives them (the session's own task's
 * first, then the latest at work). With nothing typed yet, only those of the session's own
 * task (`inTask`), and the rest appear as the user types; in a new task, which has no task of
 * its own yet, the latest few. Every word typed has to be found in the title, task, repository
 * or id.
 */
export function mentionMatches(sessions: Reachable[], query: string, inTask: boolean): Reachable[] {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    if (words.length === 0) {
        const shown = inTask ? sessions.filter((session) => session.same_task) : sessions;
        return shown.slice(0, MENTION_LIMIT);
    }
    const found = sessions.filter((session) =>
        words.every((word) => matchedOn(session).includes(word)),
    );
    return found.slice(0, MENTION_LIMIT);
}

/** Whether there are sessions to find by typing that the list does not show yet. */
export function moreToFind(sessions: Reachable[], shown: Reachable[], query: string): boolean {
    return query.trim() === "" && shown.length < sessions.length;
}

/** What the list says of where a session works: its task, its repository too where several are listed, or its chat. */
export function placeOf(session: Reachable, manyRepositories: boolean): string {
    if (session.chat) {
        return `chat ${session.task}`;
    }
    return manyRepositories ? `${session.project} / ${session.task}` : session.task;
}

/** Whether the sessions listed come from more than one repository. */
export function fromManyRepositories(sessions: Reachable[]): boolean {
    return new Set(sessions.filter((session) => !session.chat).map((s) => s.project)).size > 1;
}

/**
 * `value` with the token for `session` in place of what was typed of it, from the `@` at
 * `start` to `caret`, and a space after it; and where the caret goes, after that space.
 */
export function withMention(
    value: string,
    start: number,
    caret: number,
    session: { id: number; title: string },
) {
    const token = mentionToken(session.id, session.title);
    const after = value.slice(caret);
    const space = after.startsWith(" ") ? "" : " ";
    return {
        value: `${value.slice(0, start)}${token}${space}${after}`,
        caret: start + token.length + 1,
    };
}

/**
 * `text` with `sessions` listed under it as the core lists them for an agent that reads them
 * itself. For the sample data, which has no core to write it.
 */
export function withMentions(text: string, sessions: Mentioned[]): string {
    const trimmed = text.trim();
    if (sessions.length === 0) {
        return trimmed;
    }
    const listed = sessions.map(
        (session) => `- session ${session.id} ("${session.title.trim() || "untitled"}")`,
    );
    const list = [MENTIONED_SESSIONS, ...listed].join("\n");
    return trimmed === "" ? list : `${trimmed}\n\n${list}`;
}

/** Whether `line` opens the list of sessions under a message. */
function opensList(line: string): boolean {
    return line === MENTIONED_SESSIONS || line === MENTIONED_SESSIONS_SAID;
}

/** The sessions a list names, from its lines after the heading; what was said in each left out. */
function listedIn(lines: string[]): Mentioned[] | null {
    const listed: Mentioned[] = [];
    let inside: string | null = null;
    for (const line of lines) {
        if (inside !== null) {
            inside = line === `</session_${inside}>` ? null : inside;
            continue;
        }
        const match = LISTED.exec(line);
        if (!match) {
            return null;
        }
        const id = match[1] ?? "";
        listed.push({ id: Number(id), title: match[2] ?? "" });
        inside = line.endsWith(":") ? id : null;
    }
    return listed.length > 0 && inside === null ? listed : null;
}

/**
 * A message taken apart again: what the user wrote, and the sessions listed under it. Only a
 * list that runs to the end of the message, set apart by an empty line, is one. Attached files
 * come after it, so they are taken off first (`splitFiles`).
 */
export function splitMentions(message: string): { text: string; sessions: Mentioned[] } {
    const lines = message.split("\n");
    const at = lines.findLastIndex(opensList);
    const listed = at === -1 ? null : listedIn(lines.slice(at + 1));
    if (listed === null || (at > 0 && lines[at - 1] !== "")) {
        return { text: message, sessions: [] };
    }
    return { text: lines.slice(0, Math.max(at - 1, 0)).join("\n"), sessions: listed };
}

/**
 * A sent message's words as they can be typed again: the list under it gone, and each session
 * it names standing as a token, where it had none in the words (named on the command line,
 * say) at the end. For bringing a message back into a box.
 */
export function mentionsAgain(message: string): string {
    const { text, sessions } = splitMentions(message);
    const named = new Set(mentionedIn(text));
    const added = sessions
        .filter((session) => !named.has(session.id))
        .map((session) => mentionToken(session.id, session.title));
    if (added.length === 0) {
        return text;
    }
    return text === "" ? added.join(" ") : `${text} ${added.join(" ")}`;
}

/**
 * `text` with each token as the words of its title, or `session 42` where it shows none: what a
 * task or a session is named after, as the core names a session after its first message.
 */
export function mentionsAsWords(text: string): string {
    return text.replace(TOKEN, (_token, id: string, shown?: string) =>
        shown?.trim() ? shown.trim() : `session ${id}`,
    );
}

/** A piece of what the user wrote: words, or a session it names. */
export type MentionPiece = string | Mentioned;

/**
 * `text` in pieces: the tokens of the sessions `listed` under the message as sessions, the
 * rest as it was typed. A token the list does not name was not checked by the core, and stays
 * words.
 */
export function mentionPieces(text: string, listed: Mentioned[]): MentionPiece[] {
    const pieces: MentionPiece[] = [];
    let from = 0;
    for (const match of text.matchAll(TOKEN)) {
        const session = listed.find((one) => one.id === Number(match[1]));
        if (!session) {
            continue;
        }
        if (match.index > from) {
            pieces.push(text.slice(from, match.index));
        }
        pieces.push({ id: session.id, title: match[2] ?? session.title });
        from = match.index + match[0].length;
    }
    if (from < text.length) {
        pieces.push(text.slice(from));
    }
    return pieces;
}

/** The sessions listed under a message that no token in its words names: shown apart. */
export function mentionedApart(text: string, listed: Mentioned[]): Mentioned[] {
    const named = new Set(mentionedIn(text));
    return listed.filter((session) => !named.has(session.id));
}

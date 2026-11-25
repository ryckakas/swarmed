import type { AgentEvent } from "./api/types";
import { typedAgain } from "./githubMentions";
import { nothingWritten, tokensAgain } from "./pastes";
import { type Draft, isEmpty } from "./sending";
import { isFromSession, splitFiles } from "./transcript";

// Taking a session back to before one of its messages (#364): which messages it can go back
// to, and what goes back into the message box. Pure, so it can be tested.

/**
 * The places in `events` of the parts rewinds dropped: from the message each took the session
 * back to before, up to the rewind itself, as the core counts them (`rewind.rs`).
 */
export function droppedRanges(events: AgentEvent[]): [number, number][] {
    const messages = events.flatMap((event, at) => (event.type === "user_message" ? [at] : []));
    return events.flatMap((event, at): [number, number][] => {
        if (event.type !== "rewound") {
            return [];
        }
        const start = messages[event.to];
        return start !== undefined && start < at ? [[start, at]] : [];
    });
}

/** A message the session can be taken back to before. */
export interface RewindChoice {
    /** Its number, counting from 0 every message sent into the session. */
    number: number;
    /** The start of what the user wrote, as they typed it. */
    text: string;
}

/**
 * The messages the user sent that are still in the conversation, newest first: what `/rewind`
 * offers to go back to before. A message another session left is in the conversation too, but
 * is not the user's to edit.
 */
export function rewindChoices(events: AgentEvent[]): RewindChoice[] {
    const dropped = droppedRanges(events);
    const choices: RewindChoice[] = [];
    let number = 0;
    events.forEach((event, at) => {
        if (event.type !== "user_message") {
            return;
        }
        const counts = !dropped.some(([start, end]) => at >= start && at < end);
        if (counts && !isFromSession(event.text)) {
            choices.push({ number, text: messageWords(event.text) });
        }
        number += 1;
    });
    return choices.toReversed();
}

/** The text of message number `number` of a session, as it was sent; `null` if it has none. */
export function messageSent(events: AgentEvent[], number: number): string | null {
    const sent = events.filter((event) => event.type === "user_message")[number];
    return sent?.type === "user_message" ? sent.text : null;
}

/**
 * A sent message as the user typed it, for the message box: what it mentions as tokens, what
 * was pasted large or quoted as their tokens, and the files apart.
 */
export function draftOf(message: string): Draft {
    const { text, files } = splitFiles(message);
    return { text: tokensAgain(typedAgain(text)), files, images: [] };
}

/**
 * The message box once a message goes back into it: the message alone if the box was empty;
 * otherwise the message first and what was typed after it, so that nothing typed is lost.
 */
export function withEdited(now: Draft, message: Draft): Draft {
    if (isEmpty(now)) {
        return message;
    }
    const text = [message.text, now.text].filter((part) => part !== "").join("\n\n");
    const files = [...message.files, ...now.files.filter((file) => !message.files.includes(file))];
    return { text, files, images: now.images };
}

/** How long the start of a message may be where a rewind's line names it. */
const NAMED_CHARS = 60;

/**
 * A sent message named by its start, as the user typed it: its tokens, not what they stood for,
 * and nothing of what Swarmed listed under it. Nothing is noted for a token to send again.
 */
export function messageWords(message: string): string {
    const { text } = splitFiles(message);
    return messageNamed(tokensAgain(typedAgain(text), nothingWritten()));
}

/** A message named by its start: its first line, cut short if long. */
export function messageNamed(text: string): string {
    const line = (text.split("\n").find((one) => one.trim() !== "") ?? "").trim();
    return line.length <= NAMED_CHARS ? line : `${line.slice(0, NAMED_CHARS - 1)}…`;
}

/**
 * What a rewind's line says: where the session was taken back to, by the start of the message
 * (`named`), and whether its files went back too and were put back since.
 */
export function rewoundSaid(named: string, files: boolean, putBack: boolean): string {
    const what = files && !putBack ? "Taken back, files too," : "Taken back";
    const back = files && putBack ? " The files were put back since." : "";
    const before = named === "" ? "the message" : `“${named}”`;
    return `${what} to before ${before}.${back}`;
}

/** What a rewind's line says of commits made since, which stay on the branch. */
export function commitsSaid(commits: number | null | undefined): string | null {
    if (!commits) {
        return null;
    }
    const made = commits === 1 ? "1 commit made since stays" : `${commits} commits made since stay`;
    return `${made} on the branch: only the files went back.`;
}

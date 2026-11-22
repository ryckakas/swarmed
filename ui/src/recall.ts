// Bringing back what was sent to a session with ↑ and ↓ in its message box, as a terminal does.
// Pure, so it can be tested.

import type { AgentEvent } from "./api/types";
import type { Draft } from "./sending";
import { isFromSession, splitFiles } from "./transcript";

/** What of a key press decides whether it brings back a message. */
export interface RecallPress {
    key: string;
    metaKey: boolean;
    ctrlKey: boolean;
    altKey: boolean;
    shiftKey: boolean;
}

/** Which way a key goes through the messages sent: back to older ones, or on to newer ones. */
export type Direction = "older" | "newer";

/**
 * Which way `press` goes through the messages sent, or `null` if it is not a key that does here.
 * ↑ only with the caret on the first line and ↓ only on the last, with no selection and no
 * modifier: anywhere else they move the caret, so a message of several lines stays editable.
 */
export function recallDirection(
    press: RecallPress,
    value: string,
    start: number,
    end: number,
): Direction | null {
    if (press.metaKey || press.ctrlKey || press.altKey || press.shiftKey || start !== end) {
        return null;
    }
    if (press.key === "ArrowUp") {
        return value.lastIndexOf("\n", start - 1) === -1 ? "older" : null;
    }
    if (press.key === "ArrowDown") {
        return value.indexOf("\n", end) === -1 ? "newer" : null;
    }
    return null;
}

/**
 * The messages the user sent to a session, newest first, as they typed them: a skill's call as
 * `/name …`, not what it expanded to, and attached files apart from the text. A message another
 * session left is not the user's and is left out, and the same message sent again right after
 * itself is offered once.
 */
export function sentMessages(events: AgentEvent[]): Draft[] {
    const sent: Draft[] = [];
    for (const event of events.toReversed()) {
        if (event.type !== "user_message" || isFromSession(event.text)) {
            continue;
        }
        const message = splitFiles(event.text);
        const last = sent.at(-1);
        if (!(last && sameDraft(last, message))) {
            sent.push(message);
        }
    }
    return sent;
}

/** Whether two drafts hold the same text and the same files. */
export function sameDraft(one: Draft, other: Draft): boolean {
    return (
        one.text === other.text &&
        one.files.length === other.files.length &&
        one.files.every((file, index) => other.files[index] === file)
    );
}

/** Where going through the messages sent stands. */
export interface Browsing {
    /** The messages that can be brought back, newest first, as they were when ↑ was pressed. */
    sent: Draft[];
    /** Which of them is in the box: 0 the newest, -1 none, which is the draft. */
    at: number;
    /** What was in the box before the first ↑, which ↓ past the newest brings back. */
    draft: Draft;
    /** What the box was given last. Anything else in it means the user has typed since. */
    shown: Draft;
}

/**
 * Where going `direction` from `browsing` leads, with the box holding `now`, or `null` if the
 * key brings nothing back and is left to move the caret. A box that holds something other than
 * what browsing put there was typed in: that ends browsing, and ↑ starts again from the newest
 * message, with what is in the box now as the draft. The list is only worked out (`sentNow`)
 * when browsing starts.
 */
export function browse(
    browsing: Browsing | null,
    direction: Direction,
    now: Draft,
    sentNow: () => Draft[],
): Browsing | null {
    const going = browsing && sameDraft(browsing.shown, now) ? browsing : null;
    if (!going) {
        const sent = direction === "older" ? sentNow() : [];
        const newest = sent[0];
        return newest ? { sent, at: 0, draft: now, shown: newest } : null;
    }
    const at = going.at + (direction === "older" ? 1 : -1);
    if (at < -1) {
        return null;
    }
    const shown = at === -1 ? going.draft : going.sent[at];
    return shown ? { ...going, at, shown } : null;
}

/** Where the caret goes in a message brought back: where the next press of the same key works. */
export function caretAfter(direction: Direction, text: string): number {
    const newline = text.indexOf("\n");
    // Going back, at the end of the first line; going on, at the very end.
    return direction === "older" && newline !== -1 ? newline : text.length;
}

// A large paste stands in the message box as one short token, so the box stays readable and
// the paste can be taken out again in one stroke. When the message is sent, the token gives way
// to the text itself, wrapped so that the transcript can fold it away. Pure, except for the
// store that remembers what each token stands for until its message is sent.

/** From how many lines, or how many characters, a paste is put away behind a token. */
const LARGE_LINES = 8;
const LARGE_CHARS = 1000;

/** A token as it stands in the box: `[Pasted text #3 +68 lines]`. */
const TOKEN = /\[Pasted text #(\d+) \+(\d+) lines?\]/g;

/** A paste as the agent is sent it, and as the transcript finds it again. */
const BLOCK = /<pasted_text_(\d+)>\n([\s\S]*?)\n<\/pasted_text_\1>/g;

/** What tokens stand for, by their number. */
export type Pastes = Map<number, string>;

/** The pastes of the messages being written in this window. */
const written: Pastes = new Map();

/** Whether `text` is long enough to be put away behind a token when pasted. */
export function isLargePaste(text: string): boolean {
    return text.length >= LARGE_CHARS || text.split("\n").length >= LARGE_LINES;
}

function lineCount(text: string): number {
    return text.split("\n").length;
}

/** Remembers `text` and gives the token that stands for it. */
export function tokenFor(text: string, pastes: Pastes = written): string {
    const number = Math.max(0, ...pastes.keys()) + 1;
    pastes.set(number, text);
    const lines = lineCount(text);
    return `[Pasted text #${number} +${lines} ${lines === 1 ? "line" : "lines"}]`;
}

/** `value` with `token` in place of what is selected, and where the caret goes after it. */
export function withToken(value: string, start: number, end: number, token: string) {
    return { value: value.slice(0, start) + token + value.slice(end), caret: start + token.length };
}

/**
 * The token a deleting key takes out whole: the one that ends at the caret for Backspace, the
 * one that starts there for Delete. `null` if the caret is not at a token, or text is selected.
 */
export function tokenToRemove(
    key: string,
    value: string,
    start: number,
    end: number,
): { start: number; end: number } | null {
    if (start !== end || (key !== "Backspace" && key !== "Delete")) {
        return null;
    }
    for (const match of value.matchAll(TOKEN)) {
        const from = match.index;
        const to = from + match[0].length;
        const touched = key === "Backspace" ? to === start : from === start;
        if (touched) {
            return { start: from, end: to };
        }
    }
    return null;
}

/**
 * The message as it is sent: every token whose paste is remembered gives way to the paste,
 * wrapped in a pair of tags that carry its number. A token nobody remembers stays as typed.
 */
export function expandPastes(value: string, pastes: Pastes = written): string {
    return value.replace(TOKEN, (token, number: string) => {
        const text = pastes.get(Number(number));
        return text === undefined
            ? token
            : `<pasted_text_${number}>\n${text}\n</pasted_text_${number}>`;
    });
}

/** Forgets the pastes whose tokens are in `value`, once its message has been sent. */
export function forgetPastes(value: string, pastes: Pastes = written) {
    for (const match of value.matchAll(TOKEN)) {
        pastes.delete(Number(match[1]));
    }
}

/** A piece of a sent message: what was typed, or a paste with its number of lines. */
export type MessagePart =
    | { kind: "text"; text: string }
    | { kind: "paste"; number: number; text: string; lines: number };

/** A sent message taken apart into what was typed and what was pasted, in order. */
export function messageParts(message: string): MessagePart[] {
    const parts: MessagePart[] = [];
    let from = 0;
    for (const match of message.matchAll(BLOCK)) {
        if (match.index > from) {
            parts.push({ kind: "text", text: message.slice(from, match.index) });
        }
        const text = match[2] ?? "";
        parts.push({ kind: "paste", number: Number(match[1]), text, lines: lineCount(text) });
        from = match.index + match[0].length;
    }
    if (from < message.length) {
        parts.push({ kind: "text", text: message.slice(from) });
    }
    return parts;
}

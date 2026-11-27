// A large paste, or a quote of the agent's reply, stands in the message box as one short token,
// so the box stays readable and the token can be taken out again in one stroke. When the
// message is sent, the token gives way to the text itself, wrapped in numbered tags that the
// agent reads as what they are and the transcript finds again. Pure, except for the store that
// remembers what each token stands for until its message is sent.

import { elementSpans } from "./elements";
import { githubSpans } from "./githubMentions";
import { splitAppContexts } from "./mcpApps";
import { mentionSpans } from "./mentions";

/** From how many lines, or how many characters, a paste is put away behind a token. */
const LARGE_LINES = 8;
const LARGE_CHARS = 1000;

/** How many characters of a quote its token shows, at most. */
const QUOTE_SHOWN = 32;

/** What a token stands for: a large paste, or words quoted from one of the agent's replies. */
export type TokenKind = "paste" | "quote";

/**
 * A token as it stands in the box, of either kind: a paste's, `[Pasted text #3 +68 lines]`,
 * or a quote's, `[Quote #2: "the first words…"]`. What a quote's shows has no `"` or `]`.
 * One pattern for both, so that a message is gone through once and what a token gives way to
 * is never read again for tokens of its own.
 */
const TOKEN = /\[Pasted text #(\d+) \+\d+ lines?\]|\[Quote #(\d+): "[^"\]\n]*"\]/g;

/** Which kind of token a match of `TOKEN` is, by the group its number is in, and the number. */
function tokenOf(paste: string | undefined, quote: string | undefined) {
    const kind: TokenKind = paste === undefined ? "quote" : "paste";
    return { kind, number: Number(paste ?? quote) };
}

/** The tag a kind is sent in, with its number after it: `<quoted_reply_2>`. */
const TAGS: Record<TokenKind, string> = { paste: "pasted_text", quote: "quoted_reply" };

/** A paste or a quote as the agent is sent it, and as the transcript finds it again. */
const BLOCK = /<(pasted_text|quoted_reply)_(\d+)>\n([\s\S]*?)\n<\/\1_\2>/g;

/** What tokens stand for, by their number. */
export type Pastes = Map<number, string>;

/** What the tokens of messages being written stand for, by kind and number. */
export type Written = Record<TokenKind, Pastes>;

/** A store with nothing in it, for tokens that are only shown and never sent. */
export function nothingWritten(): Written {
    return { paste: new Map(), quote: new Map() };
}

/** The pastes and quotes of the messages being written in this window. */
const written: Written = nothingWritten();

/** Whether `text` is long enough to be put away behind a token when pasted. */
export function isLargePaste(text: string): boolean {
    return text.length >= LARGE_CHARS || text.split("\n").length >= LARGE_LINES;
}

function lineCount(text: string): number {
    return text.split("\n").length;
}

/** Remembers `text` under `wanted` if that number is free, else under the next free one. */
function remember(kept: Pastes, text: string, wanted?: number): number {
    const number =
        wanted !== undefined && !kept.has(wanted) ? wanted : Math.max(0, ...kept.keys()) + 1;
    kept.set(number, text);
    return number;
}

function pasteToken(number: number, text: string): string {
    const lines = lineCount(text);
    return `[Pasted text #${number} +${lines} ${lines === 1 ? "line" : "lines"}]`;
}

/**
 * The first words of a quote, on one line, as its token shows them: cut at a word, with `…`
 * after them when there is more, and with nothing in them that would end the token early.
 */
export function firstWords(text: string): string {
    const plain = text.replace(/\s+/g, " ").trim().replaceAll('"', "'").replaceAll("]", ")");
    if (plain.length <= QUOTE_SHOWN) {
        return plain;
    }
    // One more than shown, so that a word that ends right at the limit is kept.
    const space = plain.slice(0, QUOTE_SHOWN + 1).lastIndexOf(" ");
    return `${plain.slice(0, space > 0 ? space : QUOTE_SHOWN).trimEnd()}…`;
}

function quoteToken(number: number, text: string): string {
    return `[Quote #${number}: "${firstWords(text)}"]`;
}

const MAKE: Record<TokenKind, (number: number, text: string) => string> = {
    paste: pasteToken,
    quote: quoteToken,
};

/** Remembers a large paste and gives the token that stands for it. */
export function tokenFor(text: string, kept: Written = written): string {
    return pasteToken(remember(kept.paste, text), text);
}

/** Remembers words quoted from one of the agent's replies and gives the token for them. */
export function quoteTokenFor(words: string, kept: Written = written): string {
    return quoteToken(remember(kept.quote, words), words);
}

/** `value` with `token` in place of what is selected, and where the caret goes after it. */
export function withToken(value: string, start: number, end: number, token: string) {
    return { value: value.slice(0, start) + token + value.slice(end), caret: start + token.length };
}

/** Every token in `value`, of either kind, with where it stands and its number. */
function tokensIn(value: string) {
    return [...value.matchAll(TOKEN)].map((match) => ({
        ...tokenOf(match[1], match[2]),
        start: match.index,
        end: match.index + match[0].length,
    }));
}

/**
 * Where every token in `value` stands, of any kind: a paste's, a quote's, a mentioned
 * session's, pull request's or issue's, or a picked element's. In no particular order.
 */
export function tokenSpans(value: string): { start: number; end: number }[] {
    return [
        ...tokensIn(value),
        ...mentionSpans(value),
        ...githubSpans(value),
        ...elementSpans(value),
    ];
}

/**
 * The token a deleting key takes out whole, a paste's, a quote's, a mentioned session's, pull
 * request's or issue's, or a picked element's: the one that ends at the caret for Backspace, the one that starts there
 * for Delete. `null` if the caret is not at a token, or text is selected.
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
    const touched = tokenSpans(value).find((token) =>
        key === "Backspace" ? token.end === start : token.start === start,
    );
    return touched ? { start: touched.start, end: touched.end } : null;
}

/**
 * The message as it is sent: every token whose paste or quote is remembered gives way to it,
 * wrapped in a pair of tags that carry its number. A token nobody remembers stays as typed.
 */
export function expandTokens(value: string, kept: Written = written): string {
    return value.replace(TOKEN, (token, paste?: string, quote?: string) => {
        const { kind, number } = tokenOf(paste, quote);
        const text = kept[kind].get(number);
        const tag = `${TAGS[kind]}_${number}`;
        return text === undefined ? token : `<${tag}>\n${text}\n</${tag}>`;
    });
}

/** Forgets the pastes and quotes whose tokens are in `value`, once its message has been sent. */
export function forgetTokens(value: string, kept: Written = written) {
    for (const token of tokensIn(value)) {
        kept[token.kind].delete(token.number);
    }
}

/**
 * A message that was sent as it was typed: each paste and quote in it back behind a token,
 * remembered again under its own number if that is free. For bringing a message back into a
 * box to send again or change.
 */
export function tokensAgain(message: string, kept: Written = written): string {
    // What apps left for the agent with it was not typed, and is not brought back.
    const typed = splitAppContexts(message).text;
    return typed.replace(BLOCK, (_block, tag: string, number: string, text: string) => {
        const kind: TokenKind = tag === TAGS.quote ? "quote" : "paste";
        return MAKE[kind](remember(kept[kind], text, Number(number)), text);
    });
}

/** A piece of a sent message: what was typed, a paste with its number of lines, or a quote. */
export type MessagePart =
    | { kind: "text"; text: string }
    | { kind: "paste"; number: number; text: string; lines: number }
    | { kind: "quote"; number: number; text: string };

/**
 * A sent message taken apart into what was typed, what was pasted and what was quoted, in
 * order. A quote is shown as a block of its own, so the line break that put it on a line of
 * its own in the box is left out on either side of it.
 */
export function messageParts(message: string): MessagePart[] {
    const parts: MessagePart[] = [];
    let from = 0;
    let afterQuote = false;
    const typed = (text: string) => {
        if (text !== "") {
            parts.push({ kind: "text", text });
        }
    };
    for (const match of message.matchAll(BLOCK)) {
        const quote = match[1] === TAGS.quote;
        const before = message.slice(from, match.index);
        typed(trimmedBeside(before, afterQuote, quote));
        const number = Number(match[2]);
        const text = match[3] ?? "";
        parts.push(
            quote
                ? { kind: "quote", number, text }
                : { kind: "paste", number, text, lines: lineCount(text) },
        );
        from = match.index + match[0].length;
        afterQuote = quote;
    }
    typed(trimmedBeside(message.slice(from), afterQuote, false));
    return parts;
}

/** Text between blocks, without the one line break that sets it apart from a quote. */
function trimmedBeside(text: string, afterQuote: boolean, beforeQuote: boolean): string {
    const start = afterQuote && text.startsWith("\n") ? 1 : 0;
    const end =
        beforeQuote && text.endsWith("\n") && text.length > start ? text.length - 1 : undefined;
    return text.slice(start, end);
}

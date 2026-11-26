// The addresses in what the user wrote, so that the transcript and the queue can draw them as
// links that open in the user's browser, as a reply's links do. Pure, so it can be tested.
//
// An address starts a word with `http://`, `https://` or `www.` (opened as `https://`) and runs
// to the next space. Punctuation that ends a sentence is left out of it, and so is a closing
// bracket with no opening one inside it: `(see https://example.com/a_(b)).` gives
// `https://example.com/a_(b)`. A token (a mentioned session, pull request, issue or element, a
// paste or a quote) stays words, whatever its label holds: it stands for something else.

import { tokenSpans } from "./pastes";

/** A piece of what the user wrote: words, or an address as written and where it leads. */
export type SaidPiece = string | { address: string; shown: string };

/**
 * An address as it is found, before its end is trimmed: at the start of a word, not inside a
 * path, a mail address or a longer name, and up to a space or a character no address holds
 * unencoded.
 */
const ADDRESS = /(?<![\p{L}\p{N}_@./-])(?:https?:\/\/|www\.)[^\s<>"`]+/giu;

/** What an address needs after its start to lead anywhere: a name, a number or an IPv6 host. */
const LEADS = /^(?:https?:\/\/[\p{L}\p{N}[]|www\.[\p{L}\p{N}])/iu;

/** What ends a sentence, or closes a quote, rather than an address. */
const ENDS = new Set([".", ",", ";", ":", "!", "?", "'"]);

/** Each closing bracket, with the one that opens it. */
const OPENS: Record<string, string> = { ")": "(", "]": "[", "}": "{" };

/** `text` taken apart into words and addresses, in order, with the tokens in it left words. */
export function saidLinks(text: string): SaidPiece[] {
    const pieces: SaidPiece[] = [];
    let from = 0;
    const tokens = tokenSpans(text).sort((one, other) => one.start - other.start);
    for (const token of tokens) {
        if (token.end <= from) {
            continue;
        }
        addressesIn(text.slice(from, Math.max(from, token.start)), pieces);
        words(pieces, text.slice(Math.max(from, token.start), token.end));
        from = token.end;
    }
    addressesIn(text.slice(from), pieces);
    return pieces;
}

/** Whether anything in `pieces` is an address. */
export function hasLinks(pieces: SaidPiece[]): boolean {
    return pieces.some((piece) => typeof piece !== "string");
}

/** Adds `text`, which holds no token, to `pieces`, each address in it apart from the words. */
function addressesIn(text: string, pieces: SaidPiece[]) {
    let from = 0;
    for (const match of text.matchAll(ADDRESS)) {
        const shown = trimmed(match[0]);
        if (!LEADS.test(shown)) {
            continue;
        }
        words(pieces, text.slice(from, match.index));
        pieces.push({ address: opened(shown), shown });
        from = match.index + shown.length;
    }
    words(pieces, text.slice(from));
}

/** Adds words to `pieces`, joined to the words before them, if there are any. */
function words(pieces: SaidPiece[], text: string) {
    if (text === "") {
        return;
    }
    const last = pieces.at(-1);
    if (typeof last === "string") {
        pieces[pieces.length - 1] = last + text;
    } else {
        pieces.push(text);
    }
}

/** `found` without what ends the sentence after it or closes a bracket opened before it. */
function trimmed(found: string): string {
    let address = found;
    for (let last = address.at(-1); last !== undefined; last = address.at(-1)) {
        if (!(ENDS.has(last) || unopened(address, last))) {
            break;
        }
        address = address.slice(0, -1);
    }
    return address;
}

/** Whether `last`, the end of `address`, is a closing bracket that no bracket in it opens. */
function unopened(address: string, last: string): boolean {
    const opening = OPENS[last];
    if (opening === undefined) {
        return false;
    }
    return count(address, last) > count(address, opening);
}

/** How many times `character` stands in `text`. */
function count(text: string, character: string): number {
    return text.split(character).length - 1;
}

/** Where an address as written leads: one that starts with `www.` is on the web. */
function opened(shown: string): string {
    return /^www\./i.test(shown) ? `https://${shown}` : shown;
}

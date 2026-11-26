// What the user wrote, taken apart for drawing: the words, and apart from them the addresses,
// which open in the browser as a reply's links do, the code between backticks and the paths,
// both drawn as a reply's inline code is. Pure, so it can be tested. Not Markdown: a heading,
// emphasis or a list stays as it was typed.
//
// A token (a mentioned session, pull request, issue or element, a paste or a quote) stays
// words, whatever its label holds: it stands for something else. Code comes next, so an
// address or a path between backticks is code; then addresses, so a path in one is the
// address's (`saidPaths.ts` finds the paths).
//
// An address starts a word with `http://`, `https://` or `www.` (opened as `https://`) and runs
// to the next space. Punctuation that ends a sentence is left out of it, and so is a closing
// bracket with no opening one inside it: `(see https://example.com/a_(b)).` gives
// `https://example.com/a_(b)`.

import { tokenSpans } from "./pastes";
import { pathsIn, type SaidPath } from "./saidPaths";

/** An address as written, and where it leads. */
export interface SaidAddress {
    address: string;
    shown: string;
}

/** What the user wrote between backticks, without them. */
export interface SaidCode {
    code: string;
}

/** A piece of what the user wrote: words, an address, code or a path. */
export type SaidPiece = string | SaidAddress | SaidCode | SaidPath;

/**
 * An address as it is found, before its end is trimmed: at the start of a word, not inside a
 * path, a mail address or a longer name, and up to a space or a character no address holds
 * unencoded.
 */
const ADDRESS = /(?<![\p{L}\p{N}_@./-])(?:https?:\/\/|www\.)[^\s<>"`]+/giu;

/** What an address needs after its start to lead anywhere: a name, a number or an IPv6 host. */
const LEADS = /^(?:https?:\/\/[\p{L}\p{N}[]|www\.[\p{L}\p{N}])/iu;

/**
 * Code as Markdown has it inline: a run of backticks, then up to a run of as many, on one line.
 * A fence of three on a line of its own opens nothing, so a code block typed in a message stays
 * as it was typed.
 */
const CODE = /(?<!`)(`+)(?!`)([^\n]*?[^`\n])\1(?!`)/g;

/** What ends a sentence, or closes a quote, rather than an address. */
const ENDS = new Set([".", ",", ";", ":", "!", "?", "'"]);

/** Each closing bracket, with the one that opens it. */
const OPENS: Record<string, string> = { ")": "(", "]": "[", "}": "{" };

/** `text` taken apart into words, addresses, code and paths, in order, its tokens left words. */
export function saidPieces(text: string): SaidPiece[] {
    const pieces: SaidPiece[] = [];
    let from = 0;
    const tokens = tokenSpans(text).sort((one, other) => one.start - other.start);
    for (const token of tokens) {
        if (token.end <= from) {
            continue;
        }
        codeIn(text.slice(from, Math.max(from, token.start)), pieces);
        words(pieces, text.slice(Math.max(from, token.start), token.end));
        from = token.end;
    }
    codeIn(text.slice(from), pieces);
    return pieces;
}

/** Whether anything in `pieces` is drawn apart from the words. */
export function drawnApart(pieces: SaidPiece[]): boolean {
    return pieces.some((piece) => typeof piece !== "string");
}

/** Adds `text`, which holds no token, to `pieces`, its code apart from the rest. */
function codeIn(text: string, pieces: SaidPiece[]) {
    let from = 0;
    for (const match of text.matchAll(CODE)) {
        const code = unpadded(match[2] ?? "");
        if (code.trim() === "") {
            continue;
        }
        addressesIn(text.slice(from, match.index), pieces);
        pieces.push({ code });
        from = match.index + match[0].length;
    }
    addressesIn(text.slice(from), pieces);
}

/**
 * Code without the one space Markdown lets it have at each end, so that code which starts or
 * ends with a backtick can be written: `` `` `a` `` `` is `` `a` ``.
 */
function unpadded(code: string): string {
    return code.length > 2 && code.startsWith(" ") && code.endsWith(" ") ? code.slice(1, -1) : code;
}

/** Adds `text`, which holds no token and no code, to `pieces`, each address apart. */
function addressesIn(text: string, pieces: SaidPiece[]) {
    let from = 0;
    for (const match of text.matchAll(ADDRESS)) {
        const shown = trimmed(match[0]);
        if (!LEADS.test(shown)) {
            continue;
        }
        between(pieces, text.slice(from, match.index));
        pieces.push({ address: opened(shown), shown });
        from = match.index + shown.length;
    }
    between(pieces, text.slice(from));
}

/** Adds what lies between addresses to `pieces`, each path in it apart. */
function between(pieces: SaidPiece[], text: string) {
    for (const piece of pathsIn(text)) {
        if (typeof piece === "string") {
            words(pieces, piece);
        } else {
            pieces.push(piece);
        }
    }
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

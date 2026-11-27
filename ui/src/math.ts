// Math in what agents write: which of its delimiters count, and the form remark-math reads them
// in. remark-math reads only dollars, so `\(…\)` and `\[…\]` are written as dollars first, and
// so are single dollars where the setting lets them count. Only the text of paragraphs,
// headings and table cells is rewritten, never code, links or math already there. Pure, so it
// can be tested; `Markdown` only calls these.

import type { KatexOptions } from "katex";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import remarkParse from "remark-parse";
import { unified } from "unified";
import type { MathShown } from "./api/types";

/**
 * How KaTeX draws a formula. Nothing in one may reach anything: `trust` stays off, so `\href`,
 * `\url`, `\includegraphics` and `\html…` are drawn as their names, never as a link, an image
 * or an attribute. A size it is given is capped, and a formula it cannot read is shown as
 * written, coloured as code is (`.katex-error`).
 */
export const KATEX_OPTIONS = {
    trust: false,
    strict: "ignore",
    maxSize: 20,
    maxExpand: 1000,
    errorColor: "var(--teal)",
} satisfies KatexOptions;

/** remark-math as Swarmed reads it: `$$` and more, never a single `$`, which `marked` decides. */
export const REMARK_MATH_OPTIONS = { singleDollarTextMath: false };

/** A Markdown node, with what this file reads of it. */
interface Written {
    type: string;
    value?: string;
    position?: { start: { offset?: number }; end: { offset?: number } } | undefined;
    children?: Written[];
    data?: Record<string, unknown>;
}

/** A part of the text, from `from` up to `to`. */
interface Span {
    from: number;
    to: number;
}

/** A part of the text and what takes its place. */
interface Edit extends Span {
    put: string;
}

/** The text as it is read before it is rewritten, with the same math the drawing reads. */
const READ = unified()
    .use(remarkParse)
    .use(remarkGfm)
    .use(remarkMath, REMARK_MATH_OPTIONS)
    .freeze();

/** The blocks whose text is read for formulas. */
const PHRASING = new Set(["paragraph", "heading", "tableCell"]);

/** What inside those blocks is left as it is: code, math, HTML, and links with their addresses. */
const LEFT = new Set([
    "inlineCode",
    "inlineMath",
    "html",
    "link",
    "image",
    "linkReference",
    "imageReference",
    "footnoteReference",
]);

/** Blocks a display formula's lines must not reach into. */
const SHIELDED = new Set(["code", "math", "html"]);

/** Every node of `tree`, depth first. */
function* nodesOf(tree: Written): Generator<Written> {
    yield tree;
    for (const child of tree.children ?? []) {
        yield* nodesOf(child);
    }
}

/** Where a node is in the text, if the parser said. */
function spanOf(node: Written): Span | null {
    const from = node.position?.start.offset;
    const to = node.position?.end.offset;
    return from === undefined || to === undefined ? null : { from, to };
}

/** The spans of the nodes of `tree` whose kind is one of `kinds`. */
function spansOf(tree: Written, kinds: Set<string>): Span[] {
    return [...nodesOf(tree)]
        .filter((node) => kinds.has(node.type))
        .flatMap((node) => spanOf(node) ?? []);
}

/** Whether `at` falls inside any of `spans`. */
const inside = (at: number, spans: Span[]) => spans.some(({ from, to }) => at >= from && at < to);

/** How many backslashes stand right before `at`. */
function backslashesBefore(text: string, at: number): number {
    let count = 0;
    while (text[at - 1 - count] === "\\") {
        count += 1;
    }
    return count;
}

/** Whether the character at `at` is escaped: an odd number of backslashes before it. */
const escaped = (text: string, at: number) => backslashesBefore(text, at) % 2 === 1;

/** The longest run of `$` in `text`. */
function longestDollars(text: string): number {
    return Math.max(0, ...(text.match(/\$+/g) ?? []).map((run) => run.length));
}

/**
 * A formula in the dollars remark-math reads, on one line. Its fence is longer than any run of
 * `$` inside it, so nothing inside closes it, and says what it was written as: an even number
 * of dollars may be drawn as display, standing alone in its paragraph (`remarkMathAlone`), and an
 * odd number, from `\(…\)` or a single `$`, stays in the line.
 */
export function fenced(tex: string, display: boolean): string {
    // A line break inside would let a fence at the start of a line open a block of math, and
    // TeX reads it as a space anyway. What starts each line is the container's: `>` or spaces.
    const line = tex.replace(/\r?\n[ \t>]*/g, " ");
    let length = Math.max(display ? 2 : 3, longestDollars(line) + 1);
    if (length % 2 !== (display ? 0 : 1)) {
        length += 1;
    }
    const fence = "$".repeat(length);
    // The spaces keep a `$` at either end of the formula off the fence; the parser drops them.
    return `${fence} ${line} ${fence}`;
}

/** The index of the first unescaped `\` + `close` in `text` from `from` up to `to`, or -1. */
function delimiterClosing(text: string, from: number, to: number, close: string): number {
    for (let at = from; at < to - 1; at += 1) {
        if (text[at] === "\\" && text[at + 1] === close && !escaped(text, at)) {
            return at;
        }
    }
    return -1;
}

/**
 * The `$` that closes a single dollar opened before `from`, as Pandoc reads one, or -1: the
 * first unescaped `$`, with no space before it, no digit or `$` after it. So `$5 and $10` and
 * `costs $20` stay text, and so does `$HOME`, which nothing closes.
 */
function dollarClosing(text: string, from: number, to: number): number {
    for (let at = from; at < to; at += 1) {
        if (text[at] !== "$" || escaped(text, at)) {
            continue;
        }
        const closes = !(/\s/.test(text[at - 1] ?? " ") || /[\d$]/.test(text[at + 1] ?? ""));
        return closes ? at : -1;
    }
    return -1;
}

/** Whether a formula's edit would touch a `$` outside it, which would join the fences. */
const touchesDollar = (text: string, from: number, to: number) =>
    text[from - 1] === "$" || text[to] === "$";

/** The edit for a `\(` or `\[` at `at` within `segment`, or null when nothing closes it. */
function delimitedAt(text: string, at: number, segment: Span): Edit | null {
    const display = text[at + 1] === "[";
    const close = delimiterClosing(text, at + 2, segment.to, display ? "]" : ")");
    if (close < 0) {
        return null;
    }
    const tex = text.slice(at + 2, close);
    const to = close + 2;
    if (tex.trim() === "" || touchesDollar(text, at, to)) {
        return null;
    }
    return { from: at, to, put: fenced(tex, display) };
}

/** The edit for a single `$` at `at` within `segment`, or null when it stays a dollar sign. */
function dollarAt(text: string, at: number, segment: Span): Edit | null {
    const opens = text[at - 1] !== "$" && text[at + 1] !== "$" && !/\s/.test(text[at + 1] ?? " ");
    const close = opens ? dollarClosing(text, at + 1, segment.to) : -1;
    if (close < 0) {
        return null;
    }
    return { from: at, to: close + 1, put: fenced(text.slice(at + 1, close), false) };
}

/** The edit for a formula that starts at `at` within `segment`, if one does. */
function formulaAt(text: string, at: number, segment: Span, singleDollars: boolean): Edit | null {
    if (escaped(text, at)) {
        return null;
    }
    const next = text[at + 1];
    if (text[at] === "\\" && (next === "(" || next === "[")) {
        return delimitedAt(text, at, segment);
    }
    return text[at] === "$" && singleDollars ? dollarAt(text, at, segment) : null;
}

/**
 * `edit` with a space before it if it starts where `before` ends: `touchesDollar` reads the text
 * as written, so two formulas that touch, `\(a\)\(b\)`, would have their fences join into one
 * run of dollars, which closes neither.
 */
const apart = (edit: Edit, before: Edit | undefined): Edit =>
    before?.to === edit.from ? { ...edit, put: ` ${edit.put}` } : edit;

/** The formulas written in one stretch of a block's own text, in order. */
function formulasIn(text: string, segment: Span, singleDollars: boolean): Edit[] {
    const edits: Edit[] = [];
    let at = segment.from;
    while (at < segment.to) {
        const edit = formulaAt(text, at, segment, singleDollars);
        if (edit) {
            edits.push(apart(edit, edits.at(-1)));
        }
        // An escaped character is passed with its backslash, so `\\(` stays text.
        at = edit ? edit.to : at + (text[at] === "\\" ? 2 : 1);
    }
    return edits;
}

/** The parts of `span` that none of `left` covers. */
function stretches(span: Span, left: Span[]): Span[] {
    const within = left
        .filter(({ from, to }) => to > span.from && from < span.to)
        .sort((a, b) => a.from - b.from);
    const parts: Span[] = [];
    let from = span.from;
    for (const part of within) {
        if (part.from > from) {
            parts.push({ from, to: part.from });
        }
        from = Math.max(from, part.to);
    }
    if (from < span.to) {
        parts.push({ from, to: span.to });
    }
    return parts;
}

/** A line of the text: where it starts and ends, without its line ending. */
interface Line extends Span {
    text: string;
}

function linesOf(text: string): Line[] {
    const lines: Line[] = [];
    let from = 0;
    for (const line of text.split("\n")) {
        lines.push({ from, to: from + line.length, text: line });
        from += line.length + 1;
    }
    return lines;
}

/** A line that holds only `\[`, after what its containers put before it. */
const OPENS = /^([ \t>]*(?:(?:[-+*]|\d{1,9}[.)])[ \t]+)?)\\\[[ \t]*\r?$/;
/** A line that holds only `\]`, after its containers' marks. */
const CLOSES = /^([ \t>]*)\\\][ \t]*\r?$/;
/** A line with nothing written on it but its containers' marks. */
const BLANK = /^[ \t>]*\r?$/;

/** How deep in quotes a line's marks put it. */
const quotes = (marks: string) => marks.split(">").length - 1;

/** A `\[` or `\]` that is not escaped: `\\[2pt]`, a row's spacing, is a backslash and `[`. */
const DELIMITER = /(?<!\\)(?:\\\\)*\\[[\]]/;

/**
 * The index of the line that closes a display formula opened on line `open`, or -1. Nothing
 * between may be blank, in a code block, or hold a `\[` or `\]` of its own: a formula has none
 * of those, and a guess past them would swallow what follows.
 */
function closingLine(lines: Line[], open: number, shielded: Span[]): number {
    const depth = quotes(lines[open]?.text.match(OPENS)?.[1] ?? "");
    for (let at = open + 1; at < lines.length; at += 1) {
        const line = lines[at];
        if (!line || BLANK.test(line.text) || inside(line.from, shielded)) {
            return -1;
        }
        const closes = line.text.match(CLOSES);
        if (closes) {
            return quotes(closes[1] ?? "") === depth ? at : -1;
        }
        if (DELIMITER.test(line.text)) {
            return -1;
        }
    }
    return -1;
}

/**
 * Display formulas whose `\[` and `\]` stand on lines of their own, turned into a block of
 * math: its lines are not read as Markdown, so one starting with `-` or `+` stays in it.
 */
function displayBlocks(text: string, shielded: Span[]): { edits: Edit[]; spans: Span[] } {
    const lines = linesOf(text);
    const edits: Edit[] = [];
    const spans: Span[] = [];
    for (let at = 0; at < lines.length; at += 1) {
        const line = lines[at];
        const opens = line?.text.match(OPENS);
        if (!(line && opens) || inside(line.from, shielded)) {
            continue;
        }
        const close = closingLine(lines, at, shielded);
        const last = lines[close];
        if (!last) {
            continue;
        }
        const from = line.from + (opens[1] ?? "").length;
        const to = last.from + last.text.indexOf("\\]");
        edits.push({ from, to: from + 2, put: "$$" }, { from: to, to: to + 2, put: "$$" });
        spans.push({ from: line.from, to: last.to });
        at = close;
    }
    return { edits, spans };
}

/** Whether `text` may hold a formula `marked` would rewrite, to spare the parse when not. */
const mayHold = (text: string, singleDollars: boolean) =>
    /\\[([]/.test(text) || (singleDollars && text.includes("$"));

/**
 * An agent's text with every formula it wrote in a form remark-math reads: `\(…\)` and
 * `\[…\]`, and a single `$…$` where `singleDollars` lets it count and it reads as math. What is
 * already math, `$$…$$` and a fenced `math` block, is left alone, and so is anything in code,
 * in a link or escaped.
 */
export function marked(text: string, singleDollars: boolean): string {
    if (!mayHold(text, singleDollars)) {
        return text;
    }
    const tree = READ.parse(text) as Written;
    const { edits, spans } = displayBlocks(text, spansOf(tree, SHIELDED));
    const left = [...spansOf(tree, LEFT), ...spans];
    for (const block of spansOf(tree, PHRASING)) {
        for (const segment of stretches(block, left)) {
            edits.push(...formulasIn(text, segment, singleDollars));
        }
    }
    return edits
        .sort((a, b) => b.from - a.from)
        .reduce((done, edit) => done.slice(0, edit.from) + edit.put + done.slice(edit.to), text);
}

/** The text to draw for `text`, as the setting says formulas count. */
export function mathText(text: string, shown: MathShown): string {
    return shown === "off" ? text : marked(text, shown === "with_single_dollars");
}

/** The formula a paragraph holds and nothing else, if it does. */
function alone(paragraph: Written): Written | null {
    const written = (paragraph.children ?? []).filter(
        (child) => !(child.type === "text" && /^\s*$/.test(child.value ?? "")),
    );
    const only = written[0];
    return written.length === 1 && only?.type === "inlineMath" ? only : null;
}

/** How many `$` open the formula that starts at `from` in `source`. */
function fenceAt(source: string, from: number): number {
    return source.slice(from).match(/^\$+/)?.[0].length ?? 0;
}

/**
 * Draws a formula in an even number of dollars that stands alone in its paragraph as display,
 * as GitHub draws `$$…$$` on a line of its own; one in an odd number, written as `\(…\)` or a
 * single `$`, stays in the line (`fenced`).
 */
export function remarkMathAlone() {
    return (tree: Written, file: { value: unknown }) => {
        const source = String(file.value);
        for (const node of nodesOf(tree)) {
            const only = node.type === "paragraph" ? alone(node) : null;
            const from = only ? spanOf(only)?.from : undefined;
            if (only && from !== undefined && fenceAt(source, from) % 2 === 0) {
                only.data = {
                    ...only.data,
                    hProperties: { className: ["language-math", "math-display"] },
                };
            }
        }
    };
}

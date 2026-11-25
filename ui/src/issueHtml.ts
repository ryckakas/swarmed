// Issue text as GitHub draws it: the cleaned subset of HTML GitHub allows, narrowed further, and
// whether drawing it so leaves out anything the text holds, which the agent reads all of. Pure,
// so it can be tested; the Issues view only calls these.

import { defaultSchema, type Options as Schema } from "rehype-sanitize";
import remarkGfm from "remark-gfm";
import remarkParse from "remark-parse";
import { unified } from "unified";
import type { IssueDetail } from "./api/types";

/**
 * The elements issue text keeps: those Markdown itself makes, and HTML that only formats. Any
 * other is replaced by what it holds, or dropped with it if it is one of `DROPPED`.
 */
const KEPT = new Set([
    // What Markdown makes, GitHub's tables, task lists and footnotes included.
    ...["a", "blockquote", "br", "code", "del", "em", "hr", "img", "input", "li", "ol", "p"],
    ...["pre", "strong", "sup", "table", "tbody", "td", "tfoot", "th", "thead", "tr", "ul"],
    ...["h1", "h2", "h3", "h4", "h5", "h6"],
    // HTML that only formats.
    ...["b", "details", "i", "kbd", "s", "sub", "summary"],
]);

/** Elements dropped with all they hold: code, or a drawing, rather than text to read. */
const DROPPED = ["script", "style", "iframe", "object", "template", "noscript", "svg", "math"];

/** What a table cell keeps: its alignment, which Markdown's tables set too, and its span. */
const CELL = ["align", "colSpan", "rowSpan"];

/**
 * What issue text is cleaned with before it is drawn: `hast-util-sanitize`'s default, which
 * follows GitHub's, narrowed to the elements of `KEPT`, with no `style`, no `class` but a code
 * block's language, no `id` or `name`, no event handlers, and addresses only on the web or for
 * mail. An image keeps its address and what it shows, for the window to draw it as a link.
 */
export const ISSUE_SCHEMA: Schema = {
    ...defaultSchema,
    tagNames: (defaultSchema.tagNames ?? []).filter((tag) => KEPT.has(tag)),
    attributes: {
        a: ["href"],
        code: [["className", /^language-./]],
        details: ["open"],
        img: ["src", "alt"],
        // Only GitHub's task-list checkbox: `required` below makes any other one that.
        input: [["disabled", true], ["type", "checkbox"], "checked"],
        ol: ["start"],
        td: CELL,
        th: CELL,
    },
    protocols: { href: ["http", "https", "mailto"], src: ["http", "https"] },
    strip: DROPPED,
};

/**
 * The elements HTML written in an issue may name and be drawn whole. A checkbox written as
 * HTML is drawn as a task list's, not as written.
 */
const WRITTEN = new Set((ISSUE_SCHEMA.tagNames ?? []).filter((tag) => tag !== "input"));

// The pieces of HTML as a browser reads them. Only the spaces HTML counts as spaces: a browser
// takes any other character as part of a name.
const SPACE = "[\\t\\n\\f\\r ]";
const NAME = "[A-Za-z][A-Za-z0-9]*";
const ATTRIBUTE = `${SPACE}+([^\\t\\n\\f\\r "'>/=<\`]+)(?:${SPACE}*=${SPACE}*(?:"([^"]*)"|'([^']*)'|([^\\t\\n\\f\\r "'=<>\`]+)))?`;

/**
 * One piece of HTML read whole: text, a start tag with its attributes, or a bare end tag. A
 * comment, a declaration, an end tag with anything in it or a tag left open matches none.
 */
const PIECE = new RegExp(
    `(?<text>[^<]+|<(?![A-Za-z/!?]))|<(?<start>${NAME})(?<attributes>(?:${ATTRIBUTE})*)${SPACE}*/?>|</(?<end>${NAME})${SPACE}*>`,
    "y",
);

/** Whether `value` is a value `allowed` lets through: each word of it, for a list of words. */
function valueKept(allowed: readonly unknown[], value: string, words: boolean): boolean {
    const values = words ? value.split(new RegExp(`${SPACE}+`)).filter(Boolean) : [value];
    return values.every((one) =>
        allowed.some((rule) => (rule instanceof RegExp ? rule.test(one) : rule === one)),
    );
}

/**
 * Whether an address stays: one with no scheme, or a scheme of `protocols`, in the same case, as
 * cleaning compares them. Read conservatively, as written: a character reference or a space
 * before the first `/`, `?` or `#` could make a scheme of anything, so it counts as one that goes.
 */
function addressKept(address: string, protocols: readonly string[]): boolean {
    const scheme = /^[^/?#]*/.exec(address)?.[0] ?? "";
    if (/[&\s\p{Cc}]/u.test(scheme)) {
        return false;
    }
    const colon = scheme.indexOf(":");
    return colon === -1 || protocols.includes(scheme.slice(0, colon));
}

/** Whether `tag` written as HTML keeps its attribute `name` with `value` once cleaned. */
function attributeKept(tag: string, name: string, value: string): boolean {
    const rules = ISSUE_SCHEMA.attributes?.[tag] ?? [];
    const rule = rules.find((one) => {
        const property = (typeof one === "string" ? one : one[0]).toLowerCase();
        return property === name || (property === "classname" && name === "class");
    });
    if (rule === undefined) {
        return false;
    }
    const [property, ...allowed] = typeof rule === "string" ? [rule] : rule;
    if (allowed.length > 0 && !valueKept(allowed, value, property === "className")) {
        return false;
    }
    const protocols = ISSUE_SCHEMA.protocols?.[property];
    return protocols == null || addressKept(value, protocols);
}

/** Whether every attribute of a start tag of `tag` is kept, none of them given twice. */
function attributesKept(tag: string, written: string): boolean {
    const seen = new Set<string>();
    for (const match of written.matchAll(new RegExp(ATTRIBUTE, "g"))) {
        const name = (match[1] ?? "").toLowerCase();
        // A browser keeps the first of an attribute given twice and drops the rest unsaid.
        if (seen.has(name) || !attributeKept(tag, name, match[2] ?? match[3] ?? match[4] ?? "")) {
            return false;
        }
        seen.add(name);
    }
    return true;
}

/** Whether HTML written in an issue is drawn whole, every piece of it read and kept. */
export function htmlShown(html: string): boolean {
    PIECE.lastIndex = 0;
    while (PIECE.lastIndex < html.length) {
        const groups = PIECE.exec(html)?.groups;
        const tag = (groups?.start ?? groups?.end)?.toLowerCase();
        if (groups === undefined || (tag !== undefined && !WRITTEN.has(tag))) {
            return false;
        }
        if (groups.start !== undefined && !attributesKept(tag ?? "", groups.attributes ?? "")) {
            return false;
        }
    }
    return true;
}

/** A Markdown node, as far as finding what drawing it leaves out needs. */
interface Written {
    type: string;
    value?: string;
    url?: string;
    title?: string | null | undefined;
    identifier?: string;
    meta?: string | null | undefined;
    children?: Written[];
}

const MARKDOWN = unified().use(remarkParse).use(remarkGfm).freeze();

/** The kinds of Markdown node that use a definition, by the kind of definition they use. */
const USES: Record<string, string> = {
    linkReference: "definition",
    imageReference: "definition",
    footnoteReference: "footnoteDefinition",
};

/** Every node of `tree`, depth first. */
function* nodesOf(tree: Written): Generator<Written> {
    yield tree;
    for (const child of tree.children ?? []) {
        yield* nodesOf(child);
    }
}

/**
 * Whether one Markdown node is drawn without leaving anything of it out, given the definitions
 * something uses (as `kind:identifier`). HTML must be drawn whole; a link or an image keeps its
 * address and has no title, which is not drawn; a definition, which is never drawn itself, has
 * to be used; a code block has only its language after the fence, since no more of that line
 * is drawn.
 */
function nodeShown(node: Written, used: Set<string>): boolean {
    switch (node.type) {
        case "html":
            return htmlShown(node.value ?? "");
        case "link":
        case "image":
        case "definition": {
            const protocols = ISSUE_SCHEMA.protocols?.[node.type === "image" ? "src" : "href"];
            const kept = addressKept(node.url ?? "", protocols ?? []);
            const definitionUsed =
                node.type !== "definition" || used.has(`definition:${node.identifier}`);
            return kept && node.title == null && definitionUsed;
        }
        case "footnoteDefinition":
            return used.has(`footnoteDefinition:${node.identifier}`);
        case "code":
            return node.meta == null;
        default:
            return true;
    }
}

/**
 * Whether drawing issue text leaves any of it out: an HTML comment (GitHub hides those too), an
 * element or attribute cleaning drops, HTML that cannot be read whole, an address that goes, a
 * title, or a definition nothing uses. Conservative: HTML in code is text and counts for
 * nothing, but anything this does not read as kept counts.
 */
export function leavesOut(text: string): boolean {
    const nodes = [...nodesOf(MARKDOWN.parse(text))];
    const used = new Set(
        nodes.flatMap((node) => {
            const kind = USES[node.type];
            return kind === undefined ? [] : [`${kind}:${node.identifier}`];
        }),
    );
    return nodes.some((node) => !nodeShown(node, used));
}

/** Whether drawing an issue leaves out any of what it or a comment GitHub shows says. */
export function issueLeavesOut(issue: Pick<IssueDetail, "body" | "comments">): boolean {
    const texts = [
        issue.body,
        ...issue.comments
            .filter((comment) => comment.hidden === null)
            .map((comment) => comment.body),
    ];
    return texts.some(leavesOut);
}

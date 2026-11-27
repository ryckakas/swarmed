import { type ComponentProps, isValidElement, memo, type ReactNode, useState } from "react";
import ReactMarkdown from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import rehypeKatex from "rehype-katex";
import rehypeRaw from "rehype-raw";
import rehypeSanitize from "rehype-sanitize";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import type { MathShown } from "../api/types";
import { HTML_LANGUAGE, splitPendingView, VIEW_LANGUAGE } from "../htmlview";
import { ISSUE_SCHEMA } from "../issueHtml";
import { KATEX_OPTIONS, mathText, REMARK_MATH_OPTIONS, remarkMathAlone } from "../math";
import { useStore } from "../store";
import { HtmlView, ViewPending } from "./HtmlView";
import { Link } from "./Link";

const PLUGINS = [remarkGfm];
// Colours code blocks that name their language; one that does not is left plain, not guessed.
const REHYPE_PLUGINS = [rehypeHighlight];
// Formulas, which `mathText` has put in dollars, drawn by KaTeX before code is coloured, so a
// fenced `math` block is drawn rather than coloured. What KaTeX draws is its own markup, parsed
// in an inert template into the tree and drawn by React, never set as HTML in the page; the
// reply's own HTML stays text as it does without.
const MATH_PLUGINS = [
    remarkGfm,
    [remarkMath, REMARK_MATH_OPTIONS],
    remarkMathAlone,
] satisfies ComponentProps<typeof ReactMarkdown>["remarkPlugins"];
const MATH_REHYPE_PLUGINS = [
    [rehypeKatex, KATEX_OPTIONS],
    rehypeHighlight,
] satisfies ComponentProps<typeof ReactMarkdown>["rehypePlugins"];
// Text anyone could write: its HTML is read, then cleaned to what GitHub would draw of it, and
// only then are code blocks coloured, so that the colours' classes are not cleaned away.
const UNTRUSTED_REHYPE_PLUGINS = [
    rehypeRaw,
    [rehypeSanitize, ISSUE_SCHEMA],
    rehypeHighlight,
] satisfies ComponentProps<typeof ReactMarkdown>["rehypePlugins"];

interface Props {
    text: string;
    /** Whether the end of the text has not arrived yet. */
    streaming?: boolean;
}

/**
 * The agent's prose, with its formulas drawn as math as the settings say. Memoised on the text,
 * so a long transcript re-parses only what is new.
 */
export const Markdown = memo(function Markdown({ text, streaming = false }: Props) {
    const views = useStore((state) => state.settings?.html_views ?? false);
    const math = useStore((state) => state.settings?.math ?? "standard");
    // A view still being written is held back and stood in for, so it appears whole. With
    // views off, a view's block is the code it is.
    const { shown, pending } =
        views && streaming ? splitPendingView(text) : { shown: text, pending: false };
    return (
        <div className="prose">
            <AgentMarkdown text={shown} math={math} views={views} />
            {pending && <ViewPending />}
        </div>
    );
});

interface AgentProps {
    text: string;
    /** Which formulas are drawn as math. */
    math: MathShown;
    /** Whether a view's block is shown as a view. */
    views: boolean;
}

/** An agent's Markdown, drawn as the settings given say. */
export function AgentMarkdown({ text, math, views }: AgentProps) {
    const drawn = math !== "off";
    return (
        <ReactMarkdown
            remarkPlugins={drawn ? MATH_PLUGINS : PLUGINS}
            rehypePlugins={drawn ? MATH_REHYPE_PLUGINS : REHYPE_PLUGINS}
            components={views ? VIEW_COMPONENTS : COMPONENTS}
        >
            {mathText(text, math)}
        </ReactMarkdown>
    );
}

/** The text inside rendered Markdown, however deeply it is wrapped. */
function textOf(node: ReactNode): string {
    if (typeof node === "string" || typeof node === "number") {
        return String(node);
    }
    if (Array.isArray(node)) {
        return node.map(textOf).join("");
    }
    return isValidElement<{ children?: ReactNode }>(node) ? textOf(node.props.children) : "";
}

/** A code block: shown as a view if it is marked as one, else as the code it is. */
function Pre({ children, node: _node, ...rest }: ComponentProps<"pre"> & { node?: unknown }) {
    const code = isValidElement<{ className?: string; children?: ReactNode }>(children)
        ? children
        : null;
    const classes = code?.props.className?.split(" ") ?? [];
    if (code && classes.includes(`language-${VIEW_LANGUAGE}`)) {
        return <HtmlView html={textOf(code.props.children)} />;
    }
    if (code && classes.includes(`language-${HTML_LANGUAGE}`)) {
        return (
            <HtmlCode html={textOf(code.props.children)}>
                <pre {...rest}>{children}</pre>
            </HtmlCode>
        );
    }
    return <pre {...rest}>{children}</pre>;
}

/**
 * HTML the agent wrote as ordinary code. It stays code, since it may be quoted from a file, but
 * one press shows it as a view, for when the agent meant it to be seen.
 */
function HtmlCode({ html, children }: { html: string; children: ReactNode }) {
    const [shown, setShown] = useState(false);
    if (shown) {
        return <HtmlView html={html} />;
    }
    return (
        <div className="html-code">
            <button type="button" className="changes-refresh" onClick={() => setShown(true)}>
                Show as a view
            </button>
            {children}
        </div>
    );
}

/**
 * An image in text anyone could write: a link to it, with what it says it shows, rather than
 * loaded from wherever it points, which would tell that address that the text was read.
 */
function ImageLink({ src, alt }: ComponentProps<"img"> & { node?: unknown }) {
    const said = alt?.trim() ? `Image: ${alt.trim()}` : "Image";
    if (typeof src !== "string" || src === "") {
        return <span>[{said}]</span>;
    }
    return <Link href={src}>[{said}]</Link>;
}

/**
 * A table, in a box that scrolls sideways when the table is wider than the text around it, so
 * that the transcript never does. The table itself stays a table, as screen readers read one.
 */
function Table({ node: _node, ...rest }: ComponentProps<"table"> & { node?: unknown }) {
    return (
        <div className="prose-table">
            <table {...rest} />
        </div>
    );
}

const COMPONENTS = { a: Link, table: Table };
const VIEW_COMPONENTS = { ...COMPONENTS, pre: Pre };
const UNTRUSTED_COMPONENTS = { ...COMPONENTS, img: ImageLink };

/**
 * Text anyone could write, such as a GitHub issue and its comments, drawn as GitHub draws it:
 * Markdown, and of its HTML only what `ISSUE_SCHEMA` keeps, such as a folded `<details>` or a
 * `<kbd>`. Whatever the settings say of views, a view's block stays code with no way to show
 * it, and an image is a link: nothing in it is run or loaded.
 */
export const UntrustedMarkdown = memo(function UntrustedMarkdown({ text }: { text: string }) {
    return (
        <div className="prose">
            <ReactMarkdown
                remarkPlugins={PLUGINS}
                rehypePlugins={UNTRUSTED_REHYPE_PLUGINS}
                components={UNTRUSTED_COMPONENTS}
            >
                {text}
            </ReactMarkdown>
        </div>
    );
});

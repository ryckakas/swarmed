import { type ComponentProps, isValidElement, memo, type ReactNode, useState } from "react";
import ReactMarkdown from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import remarkGfm from "remark-gfm";
import { HTML_LANGUAGE, splitPendingView, VIEW_LANGUAGE } from "../htmlview";
import { useStore } from "../store";
import { HtmlView, ViewPending } from "./HtmlView";

const PLUGINS = [remarkGfm];
// Colours code blocks that name their language; one that does not is left plain, not guessed.
const REHYPE_PLUGINS = [rehypeHighlight];

interface Props {
    text: string;
    /** Whether the end of the text has not arrived yet. */
    streaming?: boolean;
}

/** The agent's prose. Memoised on the text, so a long transcript re-parses only what is new. */
export const Markdown = memo(function Markdown({ text, streaming = false }: Props) {
    const views = useStore((state) => state.settings?.html_views ?? false);
    // A view still being written is held back and stood in for, so it appears whole. With
    // views off, a view's block is the code it is.
    const { shown, pending } =
        views && streaming ? splitPendingView(text) : { shown: text, pending: false };
    return (
        <div className="prose">
            <ReactMarkdown
                remarkPlugins={PLUGINS}
                rehypePlugins={REHYPE_PLUGINS}
                {...(views ? { components: VIEW_COMPONENTS } : {})}
            >
                {shown}
            </ReactMarkdown>
            {pending && <ViewPending />}
        </div>
    );
});

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

const VIEW_COMPONENTS = { pre: Pre };

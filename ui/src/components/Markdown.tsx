import { memo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

const PLUGINS = [remarkGfm];

/** The agent's prose. Memoised on the text, so a long transcript re-parses only what is new. */
export const Markdown = memo(function Markdown({ text }: { text: string }) {
    return (
        <div className="prose">
            <ReactMarkdown remarkPlugins={PLUGINS}>{text}</ReactMarkdown>
        </div>
    );
});

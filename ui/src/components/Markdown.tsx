import { memo } from "react";
import ReactMarkdown from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import remarkGfm from "remark-gfm";

const PLUGINS = [remarkGfm];
// Colours code blocks that name their language; one that does not is left plain, not guessed.
const REHYPE_PLUGINS = [rehypeHighlight];

/** The agent's prose. Memoised on the text, so a long transcript re-parses only what is new. */
export const Markdown = memo(function Markdown({ text }: { text: string }) {
    return (
        <div className="prose">
            <ReactMarkdown remarkPlugins={PLUGINS} rehypePlugins={REHYPE_PLUGINS}>
                {text}
            </ReactMarkdown>
        </div>
    );
});

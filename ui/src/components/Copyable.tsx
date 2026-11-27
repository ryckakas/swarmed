import { type ReactNode, useEffect, useRef, useState } from "react";
import { CopyIcon } from "./IconButton";

/** How long the button says what came of a press, in milliseconds. */
const SAID_FOR = 1500;

/** What came of the last press, and how the button says it. */
const SAID = { copied: "Copied", refused: "Could not copy" } as const;

type Said = keyof typeof SAID;

interface Props {
    /** What a press puts on the clipboard. */
    text: string;
    /** A class of the block's own, beside `copyable`. */
    className?: string;
    children: ReactNode;
}

/**
 * A block with a button in its top right corner that copies `text`. The button floats over the
 * block, so that it never moves anything, and shows only while the pointer is over the block or
 * the keyboard is on the button.
 */
export function Copyable({ text, className, children }: Props) {
    return (
        <div className={className === undefined ? "copyable" : `copyable ${className}`}>
            {children}
            <CopyButton text={text} />
        </div>
    );
}

/** Puts `text` on the clipboard; a window with no clipboard refuses as one that says no. */
function write(text: string): Promise<void> {
    try {
        return navigator.clipboard.writeText(text);
    } catch (failed) {
        return Promise.reject(failed);
    }
}

/**
 * The button itself. It writes the clipboard when pressed and never reads it, then says for a
 * moment whether that worked. A press takes no focus and starts no selection, so it neither
 * lets go of words selected nor brings up the Quote button.
 */
function CopyButton({ text }: { text: string }) {
    const [said, setSaid] = useState<Said | null>(null);
    const timer = useRef<number | undefined>(undefined);
    useEffect(() => () => window.clearTimeout(timer.current), []);
    const say = (now: Said) => {
        window.clearTimeout(timer.current);
        setSaid(now);
        timer.current = window.setTimeout(() => setSaid(null), SAID_FOR);
    };
    // Written here, in the press itself: WebKit lets a page write the clipboard only then.
    const copy = () => {
        write(text).then(
            () => say("copied"),
            () => say("refused"),
        );
    };
    return (
        <div className="copy-corner" data-said={said ?? undefined}>
            <output className="copy-said">{said && SAID[said]}</output>
            <button
                type="button"
                className="copy-button"
                aria-label="Copy"
                title="Copy"
                onMouseDown={(event) => event.preventDefault()}
                onClick={copy}
            >
                <svg viewBox="0 0 16 16" aria-hidden="true">
                    <CopyIcon />
                </svg>
            </button>
        </div>
    );
}

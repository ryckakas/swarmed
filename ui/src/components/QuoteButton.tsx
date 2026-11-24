import { type RefObject, useEffect, useState } from "react";
import { writtenHotkey } from "../hotkeys";
import { quotedWords, quoteSpot, type Spot } from "../quotes";
import { useStore } from "../store";

/**
 * The attribute that marks one of the agent's replies as one to quote from, with the id of the
 * session whose message box a quote goes into. Only the session's own transcript sets it: not
 * a sub-agent's, whose words are not in the session agent's conversation.
 */
const QUOTE_INTO = "data-quote-into";

/** The reply of the agent's a node lies in, if it is one to quote from. */
function replyOf(node: Node): HTMLElement | null {
    const element = node instanceof Element ? node : node.parentElement;
    return element?.closest<HTMLElement>(`[${QUOTE_INTO}]`) ?? null;
}

/** What the window's selection quotes, into which session, and where it lies; `null` for none. */
function selectedQuote() {
    const selection = window.getSelection();
    if (!selection || selection.rangeCount === 0 || selection.isCollapsed) {
        return null;
    }
    const range = selection.getRangeAt(0);
    const start = replyOf(range.startContainer);
    const end = replyOf(range.endContainer);
    // The words as shown, not the reply's Markdown.
    const words = quotedWords(
        selection.toString(),
        start?.dataset.key ?? null,
        end?.dataset.key ?? null,
    );
    const sessionId = Number(start?.getAttribute(QUOTE_INTO));
    return words !== null && Number.isInteger(sessionId) ? { sessionId, words, range } : null;
}

/**
 * Quotes what is selected in one of the agent's replies into its session's message box, and
 * lets go of the selection. Says whether there was anything to quote.
 */
export function quoteSelected(): boolean {
    const quote = selectedQuote();
    if (!quote) {
        return false;
    }
    useStore.getState().quoteReply(quote.sessionId, quote.words);
    window.getSelection()?.removeAllRanges();
    return true;
}

/** About as wide and tall as the button, to keep it inside the frame. */
const BUTTON = { width: 96, height: 28 };

/** Where the button goes for what is selected now, or `null` if it quotes nothing seen here. */
function spotIn(frame: HTMLElement): Spot | null {
    const quote = selectedQuote();
    if (!(quote && frame.contains(quote.range.commonAncestorContainer))) {
        return null;
    }
    // The lines of the selection, without the empty boxes of the elements it runs through.
    const lines = [...quote.range.getClientRects()].filter((rect) => rect.width > 0);
    const whole = quote.range.getBoundingClientRect();
    return quoteSpot(
        lines[0] ?? whole,
        lines.at(-1) ?? whole,
        frame.getBoundingClientRect(),
        BUTTON,
    );
}

/**
 * Where the Quote button stands, following the selection: placed again when it changes, when
 * the pointer is let go after selecting, and when the transcript scrolls. Not while the pointer
 * is held, so that it does not chase a selection being made.
 */
function useSpot(frame: RefObject<HTMLDivElement | null>, scroller: RefObject<HTMLElement | null>) {
    const [spot, setSpot] = useState<Spot | null>(null);
    useEffect(() => {
        let held = false;
        const place = () => {
            const element = frame.current;
            setSpot(element && !held ? spotIn(element) : null);
        };
        const onDown = () => {
            held = true;
        };
        const onUp = () => {
            held = false;
            place();
        };
        const scrolling = scroller.current;
        document.addEventListener("selectionchange", place);
        document.addEventListener("pointerdown", onDown);
        document.addEventListener("pointerup", onUp);
        scrolling?.addEventListener("scroll", place, { passive: true });
        return () => {
            document.removeEventListener("selectionchange", place);
            document.removeEventListener("pointerdown", onDown);
            document.removeEventListener("pointerup", onUp);
            scrolling?.removeEventListener("scroll", place);
        };
    }, [frame, scroller]);
    return spot;
}

/**
 * The small button beside words selected in one of the agent's replies, which quotes them into
 * the message box. It is there only while the selection lies in one reply.
 */
export function QuoteButton(props: {
    frame: RefObject<HTMLDivElement | null>;
    scroller: RefObject<HTMLElement | null>;
}) {
    const spot = useSpot(props.frame, props.scroller);
    if (spot === null) {
        return null;
    }
    const shortcut = writtenHotkey("quote");
    return (
        <button
            type="button"
            className="quote-button"
            style={{ top: spot.top, left: spot.left }}
            title={`Quote these words in your message, and write what you make of them under the quote (${shortcut})`}
            // Pressing it must not let go of the selection it quotes.
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => quoteSelected()}
        >
            Quote <span className="quote-key">{shortcut}</span>
        </button>
    );
}

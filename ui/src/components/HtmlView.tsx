import { type RefObject, useEffect, useRef, useState } from "react";
import { api } from "../api";
import {
    type FrameSize,
    holdsBack,
    INITIAL_FRAME,
    needsNetwork,
    nextFrameSize,
    requestedHeight,
    requestedLink,
    type ViewTheme,
    viewDocument,
} from "../htmlview";
import { useStore } from "../store";
import { Spawning } from "./Spawning";

/**
 * A view is as tall as its content, so it never scrolls inside the conversation. This only
 * stops one that keeps asking for more, such as content sized to the frame plus a margin.
 */
const MAX_HEIGHT = 12_000;

/** The app's current colours and type, for a view to start from. */
function currentTheme(element: HTMLElement): ViewTheme {
    const style = getComputedStyle(element);
    const token = (name: string) => style.getPropertyValue(name).trim();
    return {
        text: token("--text"),
        muted: token("--muted"),
        background: token("--panel"),
        line: token("--line"),
        accent: token("--sky"),
        fontFamily: style.fontFamily,
        fontSize: style.fontSize,
        dark: style.colorScheme !== "light",
    };
}

/** What a view has told its host: how tall it is, and the link it asks to have opened. */
interface Heard {
    size: FrameSize;
    /** The address the view asks for, until the user has opened or dismissed it. */
    asked: string | null;
    settle: () => void;
}

/** Hears what a view tells its host. */
function useViewMessages(frame: RefObject<HTMLIFrameElement | null>): Heard {
    const [size, setSize] = useState(INITIAL_FRAME);
    const [asked, setAsked] = useState<string | null>(null);
    useEffect(() => {
        const onMessage = (event: MessageEvent) => {
            // Only this view's own frame may size it, or ask for one of its links to be opened.
            if (event.source !== frame.current?.contentWindow) {
                return;
            }
            const wanted = requestedHeight(event.data);
            if (wanted !== null) {
                const now = performance.now();
                setSize((current) => nextFrameSize(current, wanted, MAX_HEIGHT, now));
            }
            // A view's own script can send this without a press, with anything in the address:
            // the link is only shown, and opened when the user says so outside the view.
            const link = requestedLink(event.data);
            if (link !== null) {
                setAsked(link);
            }
        };
        window.addEventListener("message", onMessage);
        return () => window.removeEventListener("message", onMessage);
    }, [frame]);
    return { size, asked, settle: () => setAsked(null) };
}

/**
 * HTML an agent wrote, in a frame that can run its scripts and reach nothing else: it is loaded
 * from an origin of its own and sandboxed, so it cannot call the app, read the page around it,
 * open windows, submit forms or use the network.
 */
export function HtmlView({ html }: { html: string }) {
    const holder = useRef<HTMLDivElement>(null);
    const frame = useRef<HTMLIFrameElement>(null);
    const settings = useStore((state) => state.settings);
    // The document as shown, and the address the frame loads it from.
    const [shown, setShown] = useState<{ document: string; address: string } | null>(null);
    const [source, setSource] = useState(false);
    const [folded, setFolded] = useState(false);

    // biome-ignore lint/correctness/useExhaustiveDependencies: built again when the theme or text size changes
    useEffect(() => {
        const element = holder.current;
        if (!element) {
            return;
        }
        let current = true;
        const document = viewDocument(html, currentTheme(element));
        api.viewUrl(document)
            .then((address) => current && setShown({ document, address }))
            .catch(useStore.getState().report);
        return () => {
            current = false;
        };
    }, [html, settings?.theme, settings?.text_scale]);

    const { size, asked, settle } = useViewMessages(frame);

    // A view the frame no longer follows has to scroll inside it, or its end could not be reached.
    const heldBack = holdsBack(size, MAX_HEIGHT);
    useEffect(() => {
        if (heldBack) {
            frame.current?.contentWindow?.postMessage({ swarmedViewScrolls: true }, "*");
        }
    }, [heldBack]);

    const openInBrowser = () => {
        if (shown) {
            api.viewOpen(shown.document).catch(useStore.getState().report);
        }
    };

    return (
        <div className="html-view" ref={holder}>
            <div className="html-view-head">
                <button
                    type="button"
                    className="html-view-fold"
                    aria-expanded={!folded}
                    onClick={() => setFolded(!folded)}
                >
                    <svg className="fold-chevron" viewBox="0 0 10 10" aria-hidden="true">
                        <path d="M3.5 1.5 7 5 3.5 8.5" />
                    </svg>
                    View from the agent, sandboxed
                </button>
                <button type="button" className="changes-refresh" onClick={openInBrowser}>
                    Open in browser
                </button>
                <button
                    type="button"
                    className="changes-refresh"
                    onClick={() => {
                        setSource(!source);
                        setFolded(false);
                    }}
                >
                    {source ? "Show the view" : "Show the source"}
                </button>
            </div>
            {!folded && needsNetwork(html) && (
                <p className="html-view-note">
                    This view asks for files from the network, which views cannot reach. Parts of it
                    may be missing.
                </p>
            )}
            {!folded && asked !== null && (
                <p className="html-view-note">
                    This view asks to open <code>{asked}</code>{" "}
                    <button
                        type="button"
                        className="changes-refresh"
                        onClick={() => {
                            api.openLink(asked).catch(useStore.getState().report);
                            settle();
                        }}
                    >
                        Open
                    </button>{" "}
                    <button type="button" className="changes-refresh" onClick={settle}>
                        Dismiss
                    </button>
                </p>
            )}
            {!folded && source && <pre className="tool-body">{html}</pre>}
            {/* Kept while folded, so a view the user has worked in is as they left it. */}
            {!source && shown && (
                <iframe
                    ref={frame}
                    className="html-view-frame"
                    title="View from the agent"
                    sandbox="allow-scripts"
                    referrerPolicy="no-referrer"
                    src={shown.address}
                    hidden={folded}
                    style={{ height: size.height }}
                />
            )}
        </div>
    );
}

/** Stands in for a view the agent is still writing, in the place the view will take. */
export function ViewPending() {
    return (
        <div className="html-view html-view-pending" role="status">
            <div className="html-view-head">
                <span>View from the agent</span>
            </div>
            <p className="html-view-wait">
                <Spawning />
                Building the view…
            </p>
        </div>
    );
}

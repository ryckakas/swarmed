import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { needsNetwork, requestedHeight, type ViewTheme, viewDocument } from "../htmlview";
import { useStore } from "../store";

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
    const [height, setHeight] = useState(120);
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

    useEffect(() => {
        const onMessage = (event: MessageEvent) => {
            const wanted = requestedHeight(event.data);
            // Only this view's own frame may size it.
            if (wanted !== null && event.source === frame.current?.contentWindow) {
                setHeight(Math.min(wanted, MAX_HEIGHT));
            }
        };
        window.addEventListener("message", onMessage);
        return () => window.removeEventListener("message", onMessage);
    }, []);

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
                    style={{ height }}
                />
            )}
        </div>
    );
}

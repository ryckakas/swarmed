import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { requestedHeight, type ViewTheme, viewDocument } from "../htmlview";
import { useStore } from "../store";

/** The tallest a view is shown before it scrolls inside its frame. */
const MAX_HEIGHT = 720;

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
    const [address, setAddress] = useState<string | null>(null);
    const [height, setHeight] = useState(120);
    const [source, setSource] = useState(false);

    // biome-ignore lint/correctness/useExhaustiveDependencies: built again when the theme or text size changes
    useEffect(() => {
        const element = holder.current;
        if (!element) {
            return;
        }
        let current = true;
        api.viewUrl(viewDocument(html, currentTheme(element)))
            .then((url) => current && setAddress(url))
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

    return (
        <div className="html-view" ref={holder}>
            <div className="html-view-head">
                <span>View from the agent, sandboxed</span>
                <button
                    type="button"
                    className="changes-refresh"
                    onClick={() => setSource(!source)}
                >
                    {source ? "Show the view" : "Show the source"}
                </button>
            </div>
            {source && <pre className="tool-body">{html}</pre>}
            {!source && address && (
                <iframe
                    ref={frame}
                    className="html-view-frame"
                    title="View from the agent"
                    sandbox="allow-scripts"
                    referrerPolicy="no-referrer"
                    src={address}
                    style={{ height }}
                />
            )}
        </div>
    );
}

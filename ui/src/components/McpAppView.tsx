import { type RefObject, useEffect, useRef, useState } from "react";
import { sendOrQueue } from "../actions";
import { api } from "../api";
import type { AppShown } from "../api/types";
import { type FrameSize, INITIAL_FRAME, nextFrameSize } from "../htmlview";
import {
    type AppTheme,
    answer,
    consentKey,
    type Decision,
    decide,
    FIRST_APP_HEIGHT,
    failure,
    hostCapabilities,
    hostContext,
    initializeResult,
    MAX_APP_HEIGHT,
    notification,
    type Placement,
    REFUSED,
    type RpcId,
    readMessage,
    startNotifications,
    stillRunning,
    teardown,
} from "../mcpApps";
import { useStore } from "../store";
import type { AppItem } from "../transcript";

/** How often an app waiting on a tool call is told it still runs, in milliseconds. */
const STILL_RUNNING_EVERY = 20_000;

/** The app's colours and type, from the window's palette where the app is drawn. */
function themeOf(element: HTMLElement): AppTheme {
    const style = getComputedStyle(element);
    const names = ["panel", "raised", "dusk", "text", "muted", "faint", "line", "sky", "clay"];
    const tokens: Record<string, string> = {};
    for (const name of [...names, "moss", "pollen"]) {
        tokens[name] = style.getPropertyValue(`--${name}`).trim();
    }
    return {
        tokens,
        fontFamily: style.fontFamily,
        monoFamily: style.getPropertyValue("--font-mono").trim() || "ui-monospace, monospace",
        fontSize: style.fontSize,
        dark: style.colorScheme !== "light",
    };
}

/** Where the app is drawn, as it is told. */
function placementOf(element: HTMLElement, item: AppItem): Placement {
    return {
        toolUseId: item.toolUseId,
        tool: item.tool,
        width: element.clientWidth,
        locale: navigator.language,
        timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    };
}

/** What an error says, for an app to be told. */
function said(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
}

/** What an app asked that waits for the user's yes. */
type Asked =
    | { kind: "link"; id: RpcId; url: string }
    | { kind: "message"; id: RpcId; text: string };

/** Everything the bridge needs to act on one message of an app's. */
interface Bridge {
    item: AppItem;
    sessionId: number;
    holder: RefObject<HTMLDivElement | null>;
    post: (message: unknown) => void;
    /** The app (again) asks to be set up: a frame that loaded anew is told everything again. */
    starting: () => void;
    ready: () => void;
    resize: (height: number) => void;
    ask: (asked: Asked) => boolean;
}

/** Calls a tool for the app through the agent, telling the app meanwhile that it still runs. */
function callTool(bridge: Bridge, decision: Extract<Decision, { kind: "call" }>) {
    const { id, name, progress } = decision;
    let count = 0;
    const telling =
        progress === null
            ? undefined
            : setInterval(() => {
                  count += 1;
                  bridge.post(stillRunning(progress, count));
              }, STILL_RUNNING_EVERY);
    api.sessionAppCall(bridge.sessionId, bridge.item.toolUseId, name, decision.arguments)
        .then(
            (result) => bridge.post(answer(id, result)),
            (error: unknown) => bridge.post(failure(id, REFUSED, said(error))),
        )
        .finally(() => clearInterval(telling));
}

/** Answers the app's handshake with the host's context. */
function initialize(bridge: Bridge, id: RpcId, version: string) {
    const element = bridge.holder.current;
    if (element === null) {
        return;
    }
    const capabilities = hostCapabilities(bridge.item.callable.length > 0);
    const placement = placementOf(element, bridge.item);
    bridge.post(answer(id, initializeResult(version, themeOf(element), placement, capabilities)));
}

/** Does what `decision` says about one message of the app's. */
function act(bridge: Bridge, decision: Decision) {
    const { post, item, sessionId } = bridge;
    switch (decision.kind) {
        case "initialize":
            bridge.starting();
            return initialize(bridge, decision.id, decision.version);
        case "initialized":
            return bridge.ready();
        case "size":
            return bridge.resize(decision.height);
        case "answer":
            return post(answer(decision.id, decision.result));
        case "refuse":
            return post(failure(decision.id, decision.code, decision.message));
        case "context": {
            const { text } = decision;
            const context =
                text === null ? null : { toolUseId: item.toolUseId, server: item.server, text };
            useStore.getState().keepAppContext(sessionId, item.toolUseId, context);
            return post(answer(decision.id, {}));
        }
        case "call":
            return callTool(bridge, decision);
        case "link":
        case "message":
            if (!bridge.ask(decision)) {
                post(
                    failure(
                        decision.id,
                        REFUSED,
                        "Another request of this app waits for the user.",
                    ),
                );
            }
            return;
        case "ignore":
            return;
    }
}

/** What the bridge keeps between messages of an app's. */
interface Kept {
    /** The app as the transcript has it now: drawn again with every event, so not a dependency. */
    item: RefObject<AppItem>;
    /** Whether the app said it is ready. */
    ready: RefObject<boolean>;
    /** What it asked that waits for the user. */
    waiting: RefObject<Asked | null>;
}

/** The bridge for the app in `target`, which answers it and keeps what it needs in `kept`. */
function bridgeTo(
    target: Window | null,
    kept: Kept,
    shown: { holder: RefObject<HTMLDivElement | null>; sessionId: number },
    set: { size: (change: (now: FrameSize) => FrameSize) => void; asked: (one: Asked) => void },
): Bridge {
    const post = (message: unknown) => target?.postMessage(message, "*");
    const item = kept.item.current;
    return {
        item,
        sessionId: shown.sessionId,
        holder: shown.holder,
        post,
        starting: () => {
            kept.ready.current = false;
        },
        ready: () => {
            if (!kept.ready.current) {
                kept.ready.current = true;
                startNotifications(item.input, item.result).forEach(post);
            }
        },
        resize: (height) =>
            set.size((now) => nextFrameSize(now, height, MAX_APP_HEIGHT, performance.now())),
        ask: (one) => {
            if (kept.waiting.current !== null) {
                return false;
            }
            kept.waiting.current = one;
            set.asked(one);
            return true;
        },
    };
}

/** Tells an app that is ready a new theme or text size. */
function useThemeTold(
    frame: RefObject<HTMLIFrameElement | null>,
    holder: RefObject<HTMLDivElement | null>,
    kept: Kept,
) {
    const settings = useStore((state) => state.settings);
    // biome-ignore lint/correctness/useExhaustiveDependencies: told again when the theme or text size changes
    useEffect(() => {
        const element = holder.current;
        if (!kept.ready.current || element === null) {
            return;
        }
        const context = hostContext(themeOf(element), placementOf(element, kept.item.current));
        const changed = notification("ui/notifications/host-context-changed", context);
        frame.current?.contentWindow?.postMessage(changed, "*");
    }, [settings?.theme, settings?.text_scale, settings?.palette]);
}

/**
 * Listens to the app in `frame`, the only one whose messages are taken, and answers it. Once
 * it says it is ready it is told what its call was given and what came of it, and later any
 * change of the window's theme; before it goes, that it is going.
 */
function useAppBridge(
    frame: RefObject<HTMLIFrameElement | null>,
    holder: RefObject<HTMLDivElement | null>,
    item: AppItem,
    sessionId: number,
) {
    const [size, setSize] = useState<FrameSize>({ ...INITIAL_FRAME, height: FIRST_APP_HEIGHT });
    const [asked, setAsked] = useState<Asked | null>(null);
    const latest = useRef(item);
    latest.current = item;
    const kept: Kept = { item: latest, ready: useRef(false), waiting: useRef<Asked | null>(null) };
    const { ready } = kept;
    const { toolUseId } = item;

    // biome-ignore lint/correctness/useExhaustiveDependencies: one listener for the app's life, whatever the transcript draws again
    useEffect(() => {
        const target = frame.current?.contentWindow ?? null;
        const set = { size: setSize, asked: setAsked };
        const bridge = bridgeTo(target, kept, { holder, sessionId }, set);
        const onMessage = (event: MessageEvent) => {
            // Only this app's own frame is heard: no other app, view or page can speak for it.
            if (target === null || event.source !== target) {
                return;
            }
            const incoming = readMessage(event.data);
            if (incoming !== null) {
                act(bridge, decide(incoming, latest.current.callable));
            }
        };
        window.addEventListener("message", onMessage);
        return () => {
            window.removeEventListener("message", onMessage);
            if (ready.current) {
                target?.postMessage(teardown("swarmed-teardown"), "*");
            }
        };
    }, [frame, holder, sessionId, toolUseId]);
    useThemeTold(frame, holder, kept);

    const settle = (yes: boolean) => {
        const one = kept.waiting.current;
        kept.waiting.current = null;
        setAsked(null);
        if (one !== null) {
            answerAsked(one, yes, sessionId, (message) =>
                frame.current?.contentWindow?.postMessage(message, "*"),
            );
        }
    };
    return { size, asked, settle };
}

/** Does what the user said to what an app asked, and tells the app. */
function answerAsked(asked: Asked, yes: boolean, sessionId: number, post: (m: unknown) => void) {
    if (!yes) {
        const what = asked.kind === "link" ? "open the link" : "send the message";
        post(failure(asked.id, REFUSED, `The user chose not to ${what}.`));
        return;
    }
    const { report } = useStore.getState();
    if (asked.kind === "link") {
        api.openLink(asked.url).then(
            () => post(answer(asked.id, {})),
            (error: unknown) => {
                report(error);
                post(failure(asked.id, REFUSED, said(error)));
            },
        );
        return;
    }
    // Sent as shown, and alone: what apps left for the agent goes with the user's own next
    // message, where its chips are in view.
    sendOrQueue(sessionId, asked.text).then(
        () => post(answer(asked.id, {})),
        (error: unknown) => {
            report(error);
            post(failure(asked.id, REFUSED, said(error)));
        },
    );
}

/** The frame an app runs in, with what it asks the user under its head. */
function AppFrame(props: {
    item: AppItem;
    sessionId: number;
    address: string;
    hidden: boolean;
    holder: RefObject<HTMLDivElement | null>;
}) {
    const { item, sessionId, address, hidden, holder } = props;
    const frame = useRef<HTMLIFrameElement>(null);
    const { size, asked, settle } = useAppBridge(frame, holder, item, sessionId);
    return (
        <>
            {!hidden && asked !== null && <AskedNote asked={asked} settle={settle} />}
            <iframe
                ref={frame}
                className="html-view-frame"
                title={`App from ${item.server}`}
                // Scripts only: no origin of its own to keep anything in, no forms, no pop-ups,
                // and no way to send the window anywhere. The user's choice.
                sandbox="allow-scripts"
                referrerPolicy="no-referrer"
                src={address}
                hidden={hidden}
                style={{ height: size.height }}
            />
        </>
    );
}

/** What an app asks the user, with the yes and the no. */
function AskedNote({ asked, settle }: { asked: Asked; settle: (yes: boolean) => void }) {
    const yes = asked.kind === "link" ? "Open" : "Send";
    return (
        <div className="html-view-note mcp-app-asked">
            {asked.kind === "link" ? (
                <span>
                    This app asks to open <code>{asked.url}</code>
                </span>
            ) : (
                <>
                    <span>This app asks to send this to the agent:</span>
                    <blockquote>{asked.text}</blockquote>
                </>
            )}
            <span className="mcp-app-choices">
                <button type="button" className="changes-refresh" onClick={() => settle(true)}>
                    {yes}
                </button>
                <button type="button" className="changes-refresh" onClick={() => settle(false)}>
                    Dismiss
                </button>
            </span>
        </div>
    );
}

/** What the user is told about an app before or beside it: what was not kept, left out or given. */
function AppNotes({ item, shown }: { item: AppItem; shown: AppShown | null }) {
    const refused = shown?.refused ?? [];
    return (
        <>
            {item.note !== null && <p className="html-view-note">{item.note}</p>}
            {refused.length > 0 && (
                <p className="html-view-note">
                    Left out of what its server declared: {refused.join(", ")}.
                </p>
            )}
            {item.permissions.length > 0 && (
                <p className="html-view-note">
                    It asked for {item.permissions.join(", ")}, which apps are not given here.
                </p>
            )}
        </>
    );
}

/** Asks before an app that reaches the network is loaded, and names where it reaches. */
function Consent({ server, reaches }: { server: string; reaches: string[] }) {
    const allowApps = useStore((state) => state.allowApps);
    return (
        <div className="mcp-app-consent">
            <p>
                This app from the <strong>{server}</strong> server loads files from or connects to:
            </p>
            <ul>
                {reaches.map((origin) => (
                    <li key={origin}>
                        <code>{origin}</code>
                    </li>
                ))}
            </ul>
            <p className="mcp-app-consent-note">
                Showing it lets it reach these addresses, and nothing else. Your yes holds for this
                server's apps that reach the same addresses until Swarmed quits.
            </p>
            <button
                type="button"
                className="changes-refresh"
                onClick={() => allowApps(consentKey(server, reaches))}
            >
                Show
            </button>
        </div>
    );
}

/**
 * The app an MCP server attached to a tool call, drawn under the call. Its HTML is a third
 * party's: it is served from an address of its own, sandboxed, under a policy built from what
 * its server declared, and speaks to Swarmed only through the messages `mcpApps.ts` decides on.
 * One that reaches the network loads only once the user says so.
 */
export function McpAppView({ item, sessionId }: { item: AppItem; sessionId: number }) {
    const holder = useRef<HTMLDivElement>(null);
    const [folded, setFolded] = useState(false);
    const [shown, setShown] = useState<(AppShown & { address: string }) | null>(null);
    const { document, toolUseId, server } = item;
    // The transcript draws its items again with every event: what the server declared is the
    // same app only by what it says.
    const declared = JSON.stringify(item.csp);

    useEffect(() => {
        if (document === null) {
            return;
        }
        let current = true;
        api.appUrl(sessionId, toolUseId, document, JSON.parse(declared))
            .then((made) => current && setShown(made))
            .catch(useStore.getState().report);
        return () => {
            current = false;
        };
    }, [sessionId, toolUseId, document, declared]);

    const allowed = useStore(
        (state) =>
            shown !== null &&
            (shown.reaches.length === 0 ||
                state.appsAllowed.includes(consentKey(server, shown.reaches))),
    );

    return (
        <div className="html-view mcp-app" ref={holder}>
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
                    App from the {server} server, sandboxed
                </button>
            </div>
            {!folded && <AppNotes item={item} shown={shown} />}
            {!folded && shown !== null && !allowed && (
                <Consent server={server} reaches={shown.reaches} />
            )}
            {/* Kept while folded, so an app the user has worked in is as they left it. */}
            {shown !== null && allowed && (
                <AppFrame
                    item={item}
                    sessionId={sessionId}
                    address={shown.address}
                    hidden={folded}
                    holder={holder}
                />
            )}
        </div>
    );
}

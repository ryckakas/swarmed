import {
    type ClipboardEvent,
    type CompositionEvent,
    type FormEvent,
    type KeyboardEvent,
    type MouseEvent,
    type PointerEvent,
    type ReactNode,
    type RefObject,
    useEffect,
    useRef,
    useState,
    type WheelEvent,
} from "react";
import { api } from "../api";
import type {
    BrowserDisplay,
    BrowserSetup,
    BrowserState,
    BrowserTab,
    ConsoleLine,
    Task,
    Viewport,
} from "../api/types";
import {
    actorOf,
    askedToShow,
    type Box,
    BROWSER_WIDTH,
    blankPage,
    consoleCounted,
    consoleMerged,
    dialogKey,
    endsPick,
    keyInput,
    lineTime,
    mouseInput,
    type PageSize,
    restartTo,
    type Standing,
    shownAddress,
    shownTab,
    tabLabel,
    viewportOf,
    wheelInput,
} from "../browser";
import { writtenHotkey } from "../hotkeys";
import { hookKey, useStore } from "../store";
import { findSession, findSessionHome } from "../tree";
import { type Pick, useBrowser, usePick } from "../useBrowser";
import { useWords } from "../useWords";
import {
    BackIcon,
    CloseIcon,
    EndIcon,
    ForwardIcon,
    IconButton,
    OpenOutIcon,
    PickIcon,
    PlusIcon,
    PowerIcon,
    ReloadIcon,
} from "./IconButton";
import { Resizer } from "./Resizer";

/** How often the pointer's moves are passed on at most: enough for hovering, not a flood. */
const MOVE_EVERY_MS = 33;

/** How long the pane waits for its size to settle before telling Chrome. */
const RESIZE_SETTLE_MS = 120;

/** The task of the session on screen, if its Browser pane is open. */
function useBrowserTask(): Task | null {
    const opened = useStore((state) => state.browserOpen);
    const projects = useStore((state) => state.projects);
    const selected = useStore((state) => state.selectedSessionId);
    const session = findSession(projects, selected);
    const home = session && findSessionHome(projects, session);
    return home && opened.includes(home.task.id) ? home.task : null;
}

/**
 * Opens the pane when an agent of the task on screen asks to show its browser
 * (`show_browser`), on the tab it asked for. Mounted with the window, so that a closed pane
 * opens too.
 */
export function useBrowserAsked() {
    useEffect(() => {
        const subscription = api.onBrowserEvent((event) => {
            const { projects, selectedSessionId, askBrowser } = useStore.getState();
            const session = findSession(projects, selectedSessionId);
            const home = session && findSessionHome(projects, session);
            const asked = askedToShow(event, home ? home.task.id : null);
            if (asked !== null) {
                askBrowser(asked);
            }
        });
        return () => void subscription.then((unsubscribe) => unsubscribe());
    }, []);
}

/**
 * The Browser pane beside the conversation: the task's own Chrome, live. Shown while it is
 * open and a session is on screen; its edge widens it.
 */
export function BrowserColumn({
    width,
    onResize,
}: {
    width: number;
    onResize(width: number): void;
}) {
    const task = useBrowserTask();
    if (task === null) {
        return null;
    }
    return (
        <>
            <Resizer
                label="Resize the browser"
                width={width}
                limits={BROWSER_WIDTH}
                side="right"
                className="resizer-browser"
                onResize={onResize}
            />
            {/* Each task its own: its tabs, its console, what was typed in its address bar. */}
            <BrowserPane key={task.id} task={task} />
        </>
    );
}

function BrowserPane({ task }: { task: Task }) {
    const words = useWords();
    const toggleBrowser = useStore((state) => state.toggleBrowser);
    const { standing, show, retry } = useBrowser(task.id);
    const state = standing.kind === "running" ? standing.state : null;
    const tab = shownTab(state);
    const actor = useActor(tab);
    const pick = usePick(task.id, tab?.id ?? null, standing);
    const windowed = state?.windowed ?? false;
    const again = useRestart(task.id, state, retry);
    const report = useStore((store) => store.report);
    // A tab chosen here is brought forward in a window of its own, where the user sees it.
    const showTab = (id: string) => {
        show(id);
        if (windowed) {
            void api.browserRaise(task.id, id).catch(report);
        }
    };
    return (
        <aside className="browser-pane" aria-label={`The browser of ${task.name}`}>
            <div className="browser-head">
                <Tabs task={task.id} state={state} show={showTab} />
                <IconButton label="Hide the browser" onClick={() => toggleBrowser(task.id, false)}>
                    <CloseIcon />
                </IconButton>
            </div>
            {state && <Toolbar task={task.id} tab={tab} pick={pick} />}
            {actor && (
                <p className="browser-actor">
                    The browser tools of <strong>{actor}</strong> acted in this tab last.
                </p>
            )}
            {tab?.dialog && (
                // Each dialog its own: a prompt's answer is not the one before's.
                <PageDialogBar key={dialogKey(tab)} task={task.id} tab={tab} />
            )}
            {again.to === "window" && <RestartBar again={again} />}
            <Picture
                task={task.id}
                tab={tab?.id ?? null}
                onViewport={show}
                pick={pick}
                windowed={windowed}
                blank={blankPage(tab)}
            >
                {windowed ? (
                    <InItsWindow task={task.id} tab={tab?.id ?? null} again={again} />
                ) : (
                    <StandingIn standing={standing} retry={retry} task={task} />
                )}
            </Picture>
            {state && tab && <Console key={tab.id} task={task.id} tab={tab.id} />}
            {state && (
                <p className="browser-foot">
                    {state.name} with a profile of this {words.terms.task}'s own: what you sign in
                    to here stays with it. Downloads go to{" "}
                    <span className="mono">{state.downloads}</span>
                </p>
            )}
        </aside>
    );
}

/** Where the task's Chrome would start again to be shown as the settings say, and doing so. */
interface Restart {
    to: BrowserDisplay | null;
    restart(): void;
}

/**
 * The settings say to show the browser another way than the Chrome that runs was started:
 * stopping it and starting it again shows it that way. What was signed in to, and its tabs,
 * are kept.
 */
function useRestart(task: number, state: BrowserState | null, retry: () => void): Restart {
    const display = useStore((store) => store.settings?.browser_display ?? "pane");
    const report = useStore((store) => store.report);
    const to = restartTo(state, display);
    const restart = () => void api.browserStop(task).then(retry, report);
    return { to, restart };
}

/** Offers to start the browser again in a window of its own, as the settings now say. */
function RestartBar({ again }: { again: Restart }) {
    return (
        <div className="browser-dialog" role="status">
            <p className="browser-dialog-message">
                The settings show this browser in a window of its own. It runs without one until it
                starts again.
            </p>
            <div className="browser-dialog-buttons">
                <button type="button" className="button" onClick={again.restart}>
                    Start it in a window
                </button>
            </div>
        </div>
    );
}

/**
 * What stands in the page's place for a Chrome with a window of its own: Chrome draws the page
 * there, so the pane only brings that window forward.
 */
function InItsWindow(props: { task: number; tab: string | null; again: Restart }) {
    const { task, tab, again } = props;
    const words = useWords();
    const report = useStore((store) => store.report);
    return (
        <div className="browser-standing">
            <p className="browser-said">
                This {words.terms.task}'s Chrome is in a window of its own, drawn by Chrome itself.
                Browser tools act in it there, and the tabs above are its tabs.
            </p>
            <button
                type="button"
                className="button button-primary"
                onClick={() => void api.browserRaise(task, tab).catch(report)}
            >
                Show its window
            </button>
            {again.to === "pane" && (
                <button type="button" className="button" onClick={again.restart}>
                    Show it here instead
                </button>
            )}
        </div>
    );
}

/** Who last acted in `tab`, by their session's title. */
function useActor(tab: BrowserTab | null): string | null {
    const projects = useStore((state) => state.projects);
    if (tab === null) {
        return null;
    }
    return actorOf(tab, (id) => findSession(projects, id)?.title ?? null);
}

/** What stands in for the page while there is none to show. */
function StandingIn(props: { standing: Standing; retry(): void; task: Task }) {
    const { standing, retry } = props;
    switch (standing.kind) {
        case "starting":
            return <p className="browser-standing">Starting the browser…</p>;
        case "stopped":
            return (
                <div className="browser-standing">
                    <p className="browser-said">
                        The browser is stopped. What was signed in to there, and its tabs, are kept
                        for next time.
                    </p>
                    <button type="button" className="button" onClick={retry}>
                        Start it
                    </button>
                </div>
            );
        case "missing":
            return <Missing setup={standing.setup} retry={retry} />;
        case "failed":
            return (
                <div className="browser-standing" role="alert">
                    <p className="browser-said">{standing.problem}</p>
                    <button type="button" className="button" onClick={retry}>
                        Try again
                    </button>
                </div>
            );
        case "running":
            return standing.state.tabs.length === 0 ? <NoTab task={props.task.id} /> : null;
    }
}

/** No tab is open: one can be. */
function NoTab({ task }: { task: number }) {
    const report = useStore((state) => state.report);
    return (
        <div className="browser-standing">
            <p className="browser-said">No tab is open.</p>
            <button
                type="button"
                className="button"
                onClick={() => api.browserTabNew(task, null).catch(report)}
            >
                New tab
            </button>
        </div>
    );
}

/** No browser is on the machine: one can be installed, by a command shown first. */
function Missing({ setup, retry }: { setup: BrowserSetup | null; retry(): void }) {
    const [installing, setInstalling] = useState(false);
    const [problem, setProblem] = useState<string | null>(null);
    const said = useStore((state) => state.hookLines[hookKey("", "browser")]);
    const install = () => {
        setInstalling(true);
        setProblem(null);
        api.browserInstall().then(retry, (error) => {
            setProblem(String(error));
            setInstalling(false);
        });
    };
    const command = setup?.install_command ?? null;
    return (
        <div className="browser-standing">
            <p className="browser-said">
                No Google Chrome, Chromium, Microsoft Edge or Brave was found. The browser runs on
                one of them, with a profile of its own.
            </p>
            {command !== null && (
                <>
                    <p className="browser-said">
                        Swarmed can install Google Chrome with Homebrew, by running:
                    </p>
                    <pre className="mono browser-said">{command}</pre>
                    <button
                        type="button"
                        className="button"
                        disabled={installing}
                        onClick={install}
                    >
                        {installing ? "Installing…" : "Install Google Chrome"}
                    </button>
                    {installing && said && <p className="mono aside browser-said">{said}</p>}
                </>
            )}
            {command === null && (
                <p className="browser-said">
                    Install one, or set the browser's program in Settings.
                </p>
            )}
            {problem && (
                <p className="browser-said" role="alert">
                    {problem}
                </p>
            )}
        </div>
    );
}

/** The tabs, each with its title and a way to close it, and a new one. */
function Tabs({
    task,
    state,
    show,
}: {
    task: number;
    state: BrowserState | null;
    show(tab: string): void;
}) {
    const report = useStore((state) => state.report);
    if (state === null) {
        return <div className="browser-tabs" />;
    }
    const opened = () => api.browserTabNew(task, null).then(show, report);
    return (
        <div className="browser-tabs" role="tablist" aria-label="Tabs">
            {state.tabs.map((tab) => (
                <div
                    key={tab.id}
                    className="browser-tab"
                    data-shown={tab.id === state.shown}
                    data-agent={tab.acted_by !== null}
                >
                    <button
                        type="button"
                        role="tab"
                        aria-selected={tab.id === state.shown}
                        title={`${tabLabel(tab)}\n${tab.url}`}
                        onClick={() => show(tab.id)}
                    >
                        {tab.loading && <span className="browser-loading" title="Loading" />}
                        {tab.acted_by !== null && !tab.loading && (
                            <span className="browser-agent" title="An agent acted in it" />
                        )}
                        {tabLabel(tab)}
                    </button>
                    <IconButton
                        label={`Close ${tabLabel(tab)}`}
                        onClick={() => void api.browserTabClose(task, tab.id).catch(report)}
                    >
                        <CloseIcon />
                    </IconButton>
                </div>
            ))}
            <IconButton label="New tab" onClick={() => void opened()}>
                <PlusIcon />
            </IconButton>
        </div>
    );
}

/** Back, forward, load again or stop, the address, picking, and the ways out of the pane. */
function Toolbar({ task, tab, pick }: { task: number; tab: BrowserTab | null; pick: Pick }) {
    const report = useStore((state) => state.report);
    const [problem, setProblem] = useState<string | null>(null);
    if (tab === null) {
        return null;
    }
    const go = (where: Parameters<typeof api.browserGo>[2]) => {
        setProblem(null);
        api.browserGo(task, tab.id, where).catch((error) => setProblem(String(error)));
    };
    const submit = (event: FormEvent<HTMLFormElement>) => {
        event.preventDefault();
        const typed = new FormData(event.currentTarget).get("address");
        if (typeof typed === "string" && typed.trim() !== "") {
            go({ address: typed });
        }
    };
    const web = /^https?:\/\//.test(tab.url);
    return (
        <>
            <form className="browser-bar" onSubmit={submit}>
                <IconButton label="Back" disabled={!tab.can_go_back} onClick={() => go("back")}>
                    <BackIcon />
                </IconButton>
                <IconButton
                    label="Forward"
                    disabled={!tab.can_go_forward}
                    onClick={() => go("forward")}
                >
                    <ForwardIcon />
                </IconButton>
                {tab.loading ? (
                    <IconButton label="Stop loading" onClick={() => go("stop")}>
                        <EndIcon />
                    </IconButton>
                ) : (
                    <IconButton label="Load again" onClick={() => go("reload")}>
                        <ReloadIcon />
                    </IconButton>
                )}
                {/* Each tab its own: what is typed for one is not another's. */}
                <Address key={tab.id} tab={tab} />
                <IconButton
                    label={
                        pick.picking
                            ? `Stop picking (${writtenHotkey("pick-element")})`
                            : `Pick an element of the page, to point the agent at it (${writtenHotkey("pick-element")})`
                    }
                    pressed={pick.picking}
                    onClick={pick.toggle}
                >
                    <PickIcon />
                </IconButton>
                {web && (
                    <IconButton
                        label="Open in your own browser, whose cookies are not this task's"
                        onClick={() => void api.openLink(tab.url).catch(report)}
                    >
                        <OpenOutIcon />
                    </IconButton>
                )}
                <IconButton
                    label="Stop the browser. What was signed in to, and its tabs, stay for next time."
                    onClick={() => void api.browserStop(task).catch(report)}
                >
                    <PowerIcon />
                </IconButton>
            </form>
            {problem && (
                <p className="browser-problem" role="alert">
                    {problem}
                </p>
            )}
        </>
    );
}

/**
 * The address bar: where the tab is, until the user types. What is typed then stays, wherever
 * the tab goes meanwhile (a redirect ending, a page changing its own address, an agent sending
 * it on), until it is sent with Enter, given up with Esc, or left alone: the tab's address is
 * taken again only when it changes while the bar does not have the keyboard.
 */
function Address({ tab }: { tab: BrowserTab }) {
    const [typed, setTyped] = useState<string | null>(null);
    const focused = useRef(false);
    const url = tab.url;
    // biome-ignore lint/correctness/useExhaustiveDependencies: the tab going somewhere is what lets go of what was typed.
    useEffect(() => {
        if (!focused.current) {
            setTyped(null);
        }
    }, [url]);
    return (
        <input
            name="address"
            className="browser-address mono"
            aria-label="Address"
            value={shownAddress(typed, tab)}
            placeholder="An address, such as localhost:3000"
            spellCheck={false}
            autoComplete="off"
            onChange={(event) => setTyped(event.target.value)}
            onFocus={() => {
                focused.current = true;
            }}
            onBlur={() => {
                focused.current = false;
            }}
            onKeyDown={(event) => {
                if (event.key === "Escape") {
                    setTyped(null);
                } else if (event.key === "Enter") {
                    // Sent by the form; from here on the bar follows the tab again.
                    focused.current = false;
                    event.currentTarget.blur();
                }
            }}
        />
    );
}

/** A dialog the page opened, which holds it until it is answered. */
function PageDialogBar({ task, tab }: { task: number; tab: BrowserTab }) {
    const report = useStore((state) => state.report);
    const dialog = tab.dialog;
    const [text, setText] = useState(dialog?.default_prompt ?? "");
    if (dialog === null) {
        return null;
    }
    const answer = (accept: boolean) => {
        const typed = dialog.kind === "prompt" ? text : null;
        api.browserDialog(task, tab.id, accept, typed).catch(report);
    };
    return (
        <div
            className="browser-dialog"
            role="alertdialog"
            aria-label={`The page asks (${dialog.kind})`}
        >
            <p className="browser-dialog-message">{dialog.message}</p>
            {dialog.kind === "prompt" && (
                <input
                    value={text}
                    aria-label="Answer"
                    onChange={(event) => setText(event.target.value)}
                />
            )}
            <div className="browser-dialog-buttons">
                {dialog.kind !== "alert" && (
                    <button type="button" className="button" onClick={() => answer(false)}>
                        Cancel
                    </button>
                )}
                <button
                    type="button"
                    className="button button-primary"
                    onClick={() => answer(true)}
                >
                    OK
                </button>
            </div>
        </div>
    );
}

/**
 * The page, as Chrome's pictures of it, with the user's clicks, wheel and keys passed on. While
 * an element is picked, Chrome outlines what the pointer is over and takes the click itself,
 * and the page has the keyboard, so that Esc stops picking rather than the agent.
 */
function Picture(props: {
    task: number;
    tab: string | null;
    onViewport(viewport: Viewport): void;
    pick: Pick;
    /** A Chrome with a window of its own: nothing is drawn here, nor passed on from here. */
    windowed: boolean;
    /** Chrome's blank page: the white behind the picture shows instead of its black pictures. */
    blank: boolean;
    children: ReactNode;
}) {
    const { task, tab, onViewport, pick, windowed, blank } = props;
    const box = useRef<HTMLDivElement>(null);
    const picture = useRef<HTMLImageElement>(null);
    const keys = useRef<HTMLTextAreaElement>(null);
    const page = useRef<PageSize>({ width: 1, height: 1 });
    useSize(box, onViewport);
    useFrames(task, tab, picture, page);
    const input = useInput(task, box, page, keys, pick);
    useEffect(() => {
        if (pick.picking) {
            keys.current?.focus();
        }
    }, [pick.picking]);
    return (
        <div
            ref={box}
            className="browser-page"
            data-empty={tab === null || windowed}
            data-picking={pick.picking}
            {...(windowed ? {} : input.pointer)}
        >
            <img
                ref={picture}
                alt=""
                draggable={false}
                hidden={tab === null || windowed || blank}
            />
            {tab !== null && !windowed && (
                <textarea
                    ref={keys}
                    className="browser-keys"
                    aria-label="The page: what you type here goes to it"
                    {...input.keyboard}
                />
            )}
            {pick.picking && <PickingSaid windowed={windowed} />}
            {props.children}
        </div>
    );
}

/** What a click does while an element is picked, and how to stop, over the page's foot. */
function PickingSaid({ windowed }: { windowed: boolean }) {
    const words = useWords();
    const where = windowed ? "the page in its window" : "the page";
    return (
        <p className="browser-picking" role="status">
            Click an element of {where} to point the {words.terms.agent} at it. <kbd>Esc</kbd> stops
            picking.
        </p>
    );
}

/** Tells the pane's size once it settles, at the screen's density. */
function useSize(box: RefObject<HTMLDivElement | null>, onViewport: (viewport: Viewport) => void) {
    const told = useRef(onViewport);
    told.current = onViewport;
    useEffect(() => {
        const element = box.current;
        if (!element) {
            return;
        }
        let timer: ReturnType<typeof setTimeout> | undefined;
        const measure = () => {
            clearTimeout(timer);
            timer = setTimeout(() => {
                const { width, height } = element.getBoundingClientRect();
                told.current(viewportOf(width, height, window.devicePixelRatio));
            }, RESIZE_SETTLE_MS);
        };
        const observer = new ResizeObserver(measure);
        observer.observe(element);
        return () => {
            observer.disconnect();
            clearTimeout(timer);
        };
    }, [box]);
}

/** Draws each picture of the tab shown as it comes, without a render of the pane. */
function useFrames(
    task: number,
    tab: string | null,
    picture: RefObject<HTMLImageElement | null>,
    page: RefObject<PageSize>,
) {
    useEffect(() => {
        const subscription = api.onBrowserEvent((event) => {
            if (event.kind !== "frame" || event.task_id !== task || event.tab !== tab) {
                return;
            }
            page.current = { width: event.width, height: event.height };
            if (picture.current) {
                picture.current.src = `data:image/jpeg;base64,${event.data}`;
            }
        });
        return () => void subscription.then((unsubscribe) => unsubscribe());
    }, [task, tab, picture, page]);
}

/**
 * What a key on the picture does: it goes to the page, unless it is Esc while an element is
 * picked, which stops picking.
 */
function onKey(task: number, pick: Pick, kind: "down" | "up") {
    return (event: KeyboardEvent<HTMLTextAreaElement>) => {
        if (endsPick(pick.picking, event.key)) {
            event.preventDefault();
            if (kind === "down") {
                pick.end();
            }
            return;
        }
        const input = keyInput(kind, event.nativeEvent);
        if (input === null) {
            return;
        }
        event.preventDefault();
        void api.browserKey(task, input).catch(() => {});
    };
}

/** What the pointer and the keys do on the picture, passed on to the page. */
function useInput(
    task: number,
    box: RefObject<HTMLDivElement | null>,
    page: RefObject<PageSize>,
    keys: RefObject<HTMLTextAreaElement | null>,
    pick: Pick,
) {
    const moved = useRef(0);
    const where = (): Box => {
        const rect = box.current?.getBoundingClientRect();
        return rect ?? { left: 0, top: 0, width: 1, height: 1 };
    };
    const ignore = () => {};
    const send = (kind: "down" | "up" | "move", event: PointerEvent<HTMLDivElement>) =>
        void api.browserMouse(task, mouseInput(kind, event, where(), page.current)).catch(ignore);
    const pointer = {
        onPointerDown: (event: PointerEvent<HTMLDivElement>) => {
            if (!keys.current) {
                return;
            }
            event.preventDefault();
            keys.current.focus();
            event.currentTarget.setPointerCapture(event.pointerId);
            send("down", event);
        },
        onPointerUp: (event: PointerEvent<HTMLDivElement>) => keys.current && send("up", event),
        onPointerMove: (event: PointerEvent<HTMLDivElement>) => {
            if (keys.current && event.timeStamp - moved.current >= MOVE_EVERY_MS) {
                moved.current = event.timeStamp;
                send("move", event);
            }
        },
        onWheel: (event: WheelEvent<HTMLDivElement>) => {
            if (keys.current) {
                void api.browserMouse(task, wheelInput(event, where(), page.current)).catch(ignore);
            }
        },
        onContextMenu: (event: MouseEvent<HTMLDivElement>) => event.preventDefault(),
    };
    const key = (kind: "down" | "up") => onKey(task, pick, kind);
    const typed = (text: string) => text && void api.browserText(task, text).catch(ignore);
    const keyboard = {
        onKeyDown: key("down"),
        onKeyUp: key("up"),
        onPaste: (event: ClipboardEvent<HTMLTextAreaElement>) => {
            event.preventDefault();
            typed(event.clipboardData.getData("text/plain"));
        },
        onCompositionEnd: (event: CompositionEvent<HTMLTextAreaElement>) => {
            typed(event.data);
            event.currentTarget.value = "";
        },
    };
    return { pointer, keyboard };
}

/** What the tab logged and the errors it did not catch, newest last. */
function Console({ task, tab }: { task: number; tab: string }) {
    const [lines, setLines] = useState<ConsoleLine[]>([]);
    useEffect(() => {
        // Only the lines shown are held: a page that logs for hours costs no more per line.
        let held: ConsoleLine[] = [];
        const take = (more: ConsoleLine[]) => {
            held = consoleMerged(held, more);
            setLines(held);
        };
        const subscription = api.onBrowserEvent((event) => {
            if (event.kind === "console" && event.task_id === task && event.tab === tab) {
                take([event.line]);
            }
        });
        // Listening first, then reading: a line logged between the two is in one or the other.
        api.browserConsole(task, tab).then(take, () => {});
        return () => void subscription.then((unsubscribe) => unsubscribe());
    }, [task, tab]);
    const counted = consoleCounted(lines);
    return (
        <details className="browser-console">
            <summary>
                Console
                {counted.length > 0 && (
                    <span className="browser-console-count">{counted.join(", ")}</span>
                )}
            </summary>
            {lines.length === 0 ? (
                <p className="aside">Nothing logged yet.</p>
            ) : (
                <ol className="browser-console-lines mono">
                    {lines.map((line) => (
                        <li
                            key={line.n}
                            className="browser-console-line"
                            data-level={line.level}
                            title={line.source ?? undefined}
                        >
                            <time className="browser-console-time">{lineTime(line.at)}</time>
                            <span>{line.text}</span>
                        </li>
                    ))}
                </ol>
            )}
        </details>
    );
}

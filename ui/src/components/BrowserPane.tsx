import {
    type ClipboardEvent,
    type CompositionEvent,
    type FormEvent,
    type KeyboardEvent,
    type MouseEvent,
    type PointerEvent,
    type ReactNode,
    type RefObject,
    useCallback,
    useEffect,
    useRef,
    useState,
    type WheelEvent,
} from "react";
import { api } from "../api";
import type {
    BrowserSetup,
    BrowserState,
    BrowserTab,
    ConsoleLine,
    PickedElement,
    Task,
    Viewport,
} from "../api/types";
import {
    actorOf,
    askedToShow,
    type Box,
    BROWSER_WIDTH,
    consoleCounts,
    consoleMerged,
    endsPick,
    isMissing,
    keyInput,
    leftUnshown,
    lineTime,
    mouseInput,
    type PageSize,
    sameViewport,
    shownAddress,
    shownTab,
    stateOf,
    tabLabel,
    viewportOf,
    wheelInput,
} from "../browser";
import { elementToken } from "../elements";
import { hookKey, useStore } from "../store";
import { findSession, findSessionHome } from "../tree";
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

/** The task of the session on screen, if the Browser pane is open. */
function useBrowserTask(): Task | null {
    const open = useStore((state) => state.browserOpen);
    const projects = useStore((state) => state.projects);
    const selected = useStore((state) => state.selectedSessionId);
    const session = findSession(projects, selected);
    const home = session && findSessionHome(projects, session);
    return open && home ? home.task : null;
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

/** How the pane's browser stands: starting, running, or why not. */
type Standing =
    | { kind: "starting" }
    | { kind: "stopped" }
    | { kind: "running"; state: BrowserState }
    | { kind: "missing"; setup: BrowserSetup | null }
    | { kind: "failed"; problem: string };

function BrowserPane({ task }: { task: Task }) {
    const words = useWords();
    const toggleBrowser = useStore((state) => state.toggleBrowser);
    const { standing, show, retry } = useBrowser(task.id);
    const state = standing.kind === "running" ? standing.state : null;
    const tab = shownTab(state);
    const actor = useActor(tab);
    const pick = usePick(task.id, tab?.id ?? null);
    return (
        <aside className="browser-pane" aria-label={`The browser of ${task.name}`}>
            <div className="browser-head">
                <Tabs task={task.id} state={state} show={show} />
                <IconButton label="Hide the browser" onClick={() => toggleBrowser(false)}>
                    <CloseIcon />
                </IconButton>
            </div>
            {state && <Toolbar task={task.id} tab={tab} pick={pick} />}
            {actor && (
                <p className="browser-actor">
                    The browser tools of <strong>{actor}</strong> acted in this tab last.
                </p>
            )}
            {tab?.dialog && <PageDialogBar task={task.id} tab={tab} />}
            <Picture task={task.id} tab={tab?.id ?? null} onViewport={show} pick={pick}>
                <Standing standing={standing} retry={retry} task={task} />
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

/** Picking an element of the page, for the session on screen, as the pane offers it. */
interface Pick {
    /** Whether a pick is under way in this pane's browser. */
    picking: boolean;
    /** Starts a pick, or ends the one under way. */
    toggle(): void;
    /** Ends the pick under way without one. */
    end(): void;
}

/**
 * Picking an element of the tab shown, for the session on screen: started by the Pick button
 * or the shortcut, ended by a click on the page, which puts the element's token into the
 * session's message box, or by Esc, the button again, another tab, or the pane going.
 */
function usePick(task: number, tab: string | null): Pick {
    const picking = useStore((state) => state.picking?.task === task);
    const asked = useStore((state) => state.pickAsked);
    // Which pick is the latest: one that ends after another began says nothing of it.
    const runs = useRef(0);
    const end = useCallback(() => {
        void api.browserPickEnd(task).catch(useStore.getState().report);
    }, [task]);
    const start = useCallback(() => {
        if (tab !== null) {
            startPick(task, runs);
        }
    }, [task, tab]);
    // The shortcut asked for a pick: it starts once there is a tab to pick in.
    useEffect(() => {
        if (asked && tab !== null) {
            useStore.getState().askToPick(false);
            start();
        }
    }, [asked, tab, start]);
    // The pane going ends a pick under way: nothing would show where to click.
    useEffect(
        () => () => {
            if (useStore.getState().picking?.task === task) {
                void api.browserPickEnd(task).catch(() => {});
            }
        },
        [task],
    );
    return { picking, toggle: () => (picking ? end() : start()), end };
}

/**
 * Puts the page of `task` into Chrome's inspect mode for the session on screen, and the token
 * of what is clicked into that session's message box. `runs` counts the picks: one that ends
 * after another began says nothing of whether a pick is under way.
 */
function startPick(task: number, runs: RefObject<number>) {
    const { selectedSessionId: session, notePicking, report } = useStore.getState();
    if (session === null) {
        return;
    }
    runs.current += 1;
    const run = runs.current;
    notePicking({ task, session });
    const placed = (element: PickedElement | null) =>
        element &&
        useStore.getState().placePick(session, elementToken(element.number, element.label));
    api.browserPick(task, session)
        .then(placed, report)
        .finally(() => runs.current === run && useStore.getState().notePicking(null));
}

/** Who last acted in `tab`, by their session's title. */
function useActor(tab: BrowserTab | null): string | null {
    const projects = useStore((state) => state.projects);
    if (tab === null) {
        return null;
    }
    return actorOf(tab, (id) => findSession(projects, id)?.title ?? null);
}

/**
 * Starts the task's browser as the pane opens, follows what it says, and shows its tab at the
 * pane's size, or the tab an agent asked to show. The pane going off screen asks for no more
 * pictures.
 */
function useBrowser(task: number) {
    const [standing, setStanding] = useState<Standing>({ kind: "starting" });
    const viewport = useRef<Viewport | null>(null);
    // Whether the pane may start the browser by itself: not once it stopped or could not
    // start, until the user asks again. A resize then only remembers the size.
    const active = useRef(true);
    const settle = useCallback((next: Standing) => {
        active.current = next.kind === "running" || next.kind === "starting";
        setStanding(next);
    }, []);
    const showTab = (tab: string | null) => {
        if (active.current && viewport.current) {
            void showing(task, tab, viewport.current, settle);
        }
    };
    useStandingHeard(task, (state) => {
        settle(state.running ? { kind: "running", state } : { kind: "stopped" });
        // Started behind a stopped pane by an agent's tools, or its shown tab closed: it shows
        // one of the tabs it has. Not while the window is hidden, when no pictures are asked.
        if (leftUnshown(state) && document.visibilityState === "visible") {
            showTab(null);
        }
    });
    useHiddenWithWindow(task, () => showTab(null));
    // Asked for by an agent, the browser starts again even where the user had stopped it.
    const resized = useRoomAndAsking(task, viewport, showTab, () => {
        active.current = true;
    });
    const show = (wanted: Viewport | string) =>
        typeof wanted === "string" ? showTab(wanted) : resized(wanted);
    const retry = () => {
        settle({ kind: "starting" });
        showTab(null);
    };
    return { standing, show, retry };
}

/**
 * Shows the page again when the pane's room is first known or changes, and the tab an agent
 * asked for: at once, or once the room is known. Returns what to call with the room.
 */
function useRoomAndAsking(
    task: number,
    viewport: RefObject<Viewport | null>,
    showTab: (tab: string | null) => void,
    wake: () => void,
) {
    const waiting = useRef<string | null>(null);
    useAskedTab(task, (tab) => {
        wake();
        if (viewport.current) {
            showTab(tab);
        } else {
            waiting.current = tab;
        }
    });
    return (view: Viewport) => {
        const tab = waiting.current;
        waiting.current = null;
        if (sameViewport(viewport.current, view) && tab === null) {
            return;
        }
        viewport.current = view;
        showTab(tab);
    };
}

/** Goes to the tab an agent asked to show in this task, once, and forgets the asking. */
function useAskedTab(task: number, go: (tab: string) => void) {
    const asked = useStore((state) => state.browserAsked);
    const askBrowser = useStore((state) => state.askBrowser);
    const going = useRef(go);
    going.current = go;
    useEffect(() => {
        if (asked === null || asked.task !== task) {
            return;
        }
        askBrowser(null);
        if (asked.tab !== null) {
            going.current(asked.tab);
        }
    }, [asked, task, askBrowser]);
}

/** Follows what the core says of the task's browser: its tabs, and whether it runs. */
function useStandingHeard(task: number, heard: (state: BrowserState) => void) {
    const told = useRef(heard);
    told.current = heard;
    useEffect(() => {
        const subscription = api.onBrowserEvent((event) => {
            const state = stateOf(event, task);
            if (state !== null) {
                told.current(state);
            }
        });
        return () => void subscription.then((unsubscribe) => unsubscribe());
    }, [task]);
}

/**
 * No pictures are asked for while the window is hidden or the pane is gone; the window shown
 * again shows the page again.
 */
function useHiddenWithWindow(task: number, again: () => void) {
    const showAgain = useRef(again);
    showAgain.current = again;
    useEffect(() => {
        const hide = () => void api.browserHide(task).catch(() => {});
        const changed = () =>
            document.visibilityState === "hidden" ? hide() : showAgain.current();
        document.addEventListener("visibilitychange", changed);
        return () => {
            document.removeEventListener("visibilitychange", changed);
            hide();
        };
    }, [task]);
}

/** Shows a tab of the task's browser, starting it if need be, and says how that went. */
async function showing(
    task: number,
    tab: string | null,
    viewport: Viewport,
    setStanding: (standing: Standing) => void,
) {
    try {
        const state = await api.browserShow(task, tab, viewport);
        setStanding({ kind: "running", state });
    } catch (error) {
        const problem = String(error);
        if (isMissing(problem)) {
            const setup = await api.browserSetup().catch(() => null);
            setStanding({ kind: "missing", setup });
        } else {
            setStanding({ kind: "failed", problem });
        }
    }
}

/** What stands in for the page while there is none to show. */
function Standing(props: { standing: Standing; retry(): void; task: Task }) {
    const { standing, retry } = props;
    switch (standing.kind) {
        case "starting":
            return <p className="browser-standing">Starting the browser…</p>;
        case "stopped":
            return (
                <div className="browser-standing">
                    <p className="browser-said">
                        The browser is stopped. What was signed in to there is kept for next time.
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
                            ? "Stop picking (⌘⇧C)"
                            : "Pick an element of the page, to point the agent at it (⌘⇧C)"
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
                    label="Stop the browser. What was signed in to stays for next time."
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
    children: ReactNode;
}) {
    const { task, tab, onViewport, pick } = props;
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
            data-empty={tab === null}
            data-picking={pick.picking}
            {...input.pointer}
        >
            <img ref={picture} alt="" draggable={false} hidden={tab === null} />
            {tab !== null && (
                <textarea
                    ref={keys}
                    className="browser-keys"
                    aria-label="The page: what you type here goes to it"
                    {...input.keyboard}
                />
            )}
            {pick.picking && <PickingSaid />}
            {props.children}
        </div>
    );
}

/** What a click does while an element is picked, and how to stop, over the page's foot. */
function PickingSaid() {
    const words = useWords();
    return (
        <p className="browser-picking" role="status">
            Click an element of the page to point the {words.terms.agent} at it. <kbd>Esc</kbd>{" "}
            stops picking.
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
        const heard: ConsoleLine[] = [];
        let kept: ConsoleLine[] = [];
        const subscription = api.onBrowserEvent((event) => {
            if (event.kind === "console" && event.task_id === task && event.tab === tab) {
                heard.push(event.line);
                setLines(consoleMerged(kept, heard));
            }
        });
        // Listening first, then reading: a line logged between the two is in one or the other.
        api.browserConsole(task, tab).then(
            (read) => {
                kept = read;
                setLines(consoleMerged(kept, heard));
            },
            () => {},
        );
        return () => void subscription.then((unsubscribe) => unsubscribe());
    }, [task, tab]);
    const { errors, warnings } = consoleCounts(lines);
    const counted = [errors && `${errors} errors`, warnings && `${warnings} warnings`].filter(
        Boolean,
    );
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

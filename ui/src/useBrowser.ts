// The Browser pane's hooks: starting the task's browser as the pane opens, following what the
// core says of it, showing a tab at the pane's size, and picking an element of the page. What
// they decide is in `browser.ts`, where it is tested; here it is only acted on.

import { type RefObject, useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import type { BrowserState, PickedElement, Viewport } from "./api/types";
import {
    isMissing,
    leftUnshown,
    pickAsked,
    type Standing,
    sameViewport,
    standingOf,
    stateOf,
} from "./browser";
import { elementToken } from "./elements";
import { useStore } from "./store";

/** Picking an element of the page, for the session on screen, as the pane offers it. */
export interface Pick {
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
export function usePick(task: number, tab: string | null, standing: Standing): Pick {
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
    // The shortcut asked for a pick: it starts once there is a tab to pick in, and is let go
    // of where none is coming. Left standing, it would start a pick by itself, in whichever
    // pane next showed a tab, and take the next click there.
    const asking = asked ? pickAsked(tab, standing) : "wait";
    useEffect(() => {
        if (asking !== "wait") {
            useStore.getState().askToPick(false);
        }
        if (asking === "start") {
            start();
        }
    }, [asking, start]);
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

/**
 * Starts the task's browser as the pane opens, follows what it says, and shows its tab at the
 * pane's size, or the tab an agent asked to show. The pane going off screen asks for no more
 * pictures.
 */
export function useBrowser(task: number) {
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
        settle(standingOf(state));
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

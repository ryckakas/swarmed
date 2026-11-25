import { api } from "./api";
import {
    moveTarget,
    type OrderTarget,
    type PinTarget,
    pinnable,
    pinnedNow,
    pinTarget,
    targetOf,
} from "./ordering";
import { type Attached, delivered, NOTHING_ATTACHED } from "./sending";
import { standing } from "./setup";
import { useStore } from "./store";
import { messageNumber } from "./transcript";
import { findSession, findSessionHome } from "./tree";

// Things the user can ask for from more than one place: a button, and a keyboard shortcut.

/**
 * Sends a message to a session, or has it wait in the session's queue while its agent is at
 * work or waiting for an answer, or its task is being set up: whatever sends a message, a
 * button as much as the message box, must not take an open request away. With `atOnce` it does not wait: for a command
 * Swarmed does itself. With `now` the turn under way is stopped and the message goes ahead of
 * what waits. Rejects if the message could not be handed over.
 */
export function sendOrQueue(
    id: number,
    text: string,
    attached: Attached = NOTHING_ATTACHED,
    atOnce = false,
    now = false,
): Promise<void> {
    const { projects } = useStore.getState();
    const session = findSession(projects, id);
    const task = session && findSessionHome(projects, session)?.task;
    // While its task is set up, a session waits as a busy one does: what is typed goes after.
    return delivered(api, standing(session?.status, task), id, text, attached, atOnce, now);
}

/** Moves a row of the sidebar `by` places among its siblings, then reads the tree again. */
export async function moveRow(target: OrderTarget, by: number) {
    const { refresh, report } = useStore.getState();
    try {
        await moveTarget(api, target, by);
        await refresh(api);
    } catch (error) {
        report(error);
    }
}

/** The row of the sidebar written as `target`, if it is on screen. */
function rowOf(target: OrderTarget): Element | undefined {
    const written = JSON.stringify(target);
    const rows = document.querySelectorAll(".sidebar [data-order]");
    return [...rows].find((row) => row.getAttribute("data-order") === written);
}

/** Pins a row of the sidebar above its siblings, or unpins it, then reads the tree again. */
export async function pinRow(target: PinTarget, pinned: boolean) {
    const { refresh, report } = useStore.getState();
    try {
        await pinTarget(api, target, pinned);
        await refresh(api);
    } catch (error) {
        report(error);
    }
}

/**
 * The row of the sidebar that has the keyboard, with what in it has the keyboard, or the
 * session on screen when no row has it.
 */
function rowInHand(): { target: OrderTarget; had: Element | null } | null {
    const had = document.activeElement;
    const focused = had?.closest(".sidebar [data-order]");
    if (focused) {
        const target = targetOf(focused);
        return target && { target, had };
    }
    const { selectedSessionId } = useStore.getState();
    return selectedSessionId === null
        ? null
        : { target: { kind: "session", id: selectedSessionId }, had: null };
}

/**
 * Gives the keyboard back to a row that was moved. Moving the row's element on the page takes
 * the keyboard from it. It goes back to what had it, or else to the row's own button, the
 * first it has: the name that folds a repository or a task, or a session's or cloud task's
 * row. The grip it is dragged by cannot take the keyboard.
 */
function keepKeyboard(target: OrderTarget, had: Element) {
    requestAnimationFrame(() => {
        const own = rowOf(target)?.querySelector<HTMLElement>("button");
        (had instanceof HTMLElement && had.isConnected ? had : own)?.focus();
    });
}

/**
 * Moves the row of the sidebar that has the keyboard `by` places among its siblings, or the
 * session on screen when no row has it. A row keeps the keyboard as it moves.
 */
export async function moveFocusedRow(by: number) {
    const inHand = rowInHand();
    if (!inHand) {
        return;
    }
    await moveRow(inHand.target, by);
    if (inHand.had) {
        keepKeyboard(inHand.target, inHand.had);
    }
}

/**
 * Pins the row of the sidebar that has the keyboard, or the session on screen when no row has
 * it, or unpins it if it is pinned. A row keeps the keyboard as it moves.
 */
export async function pinFocusedRow() {
    const inHand = rowInHand();
    const target = inHand?.target;
    if (!(inHand && target && pinnable(target))) {
        return;
    }
    const pinned = pinnedNow(useStore.getState().projects, target);
    if (pinned === null) {
        return;
    }
    await pinRow(target, !pinned);
    if (inHand.had) {
        keepKeyboard(target, inHand.had);
    }
}

/** Asks for a folder and registers the repository it is in. */
export async function addProject() {
    const { refresh, report } = useStore.getState();
    try {
        const path = await api.pickFolder();
        if (path !== null) {
            await api.projectAdd(path);
            await refresh(api);
        }
    } catch (error) {
        report(error);
    }
}

/**
 * Starts a new session from what was said in another, from its message number `from` on if
 * given, and opens it. With `agent`, the new session runs on that agent instead of the first
 * one's: a clone of the conversation for another agent to carry on.
 */
export async function forkSession(id: number, from: number | null, agent?: string) {
    const { refresh, open, report } = useStore.getState();
    try {
        const fork = await api.sessionFork(id, from, agent);
        await refresh(api);
        await open(api, fork.id);
    } catch (error) {
        report(error);
    }
}

/**
 * Forks session `id` from the message shown for its event `key`: the new session holds that
 * message and what was said after it, and nothing from before.
 */
export function forkFromMessage(id: number, key: number) {
    const events = useStore.getState().events[id] ?? [];
    return forkSession(id, messageNumber(events, key));
}

/** Creates a session in a task, with the defaults from the settings, and opens it. */
export async function newSession(project: string, task: string) {
    const { refresh, open, report, settings } = useStore.getState();
    try {
        const session = await api.sessionNew(project, task, {
            agent: null,
            model: null,
            supervision: settings?.default_supervision ?? "auto",
            effort: null,
        });
        await refresh(api);
        await open(api, session.id);
    } catch (error) {
        report(error);
    }
}

import { api } from "./api";
import { moveTarget, type OrderTarget, targetOf } from "./ordering";
import { delivered } from "./sending";
import { useStore } from "./store";
import { messageNumber } from "./transcript";
import { findSession } from "./tree";

// Things the user can ask for from more than one place: a button, and a keyboard shortcut.

/**
 * Sends a message to a session, or has it wait in the session's queue while its agent is at
 * work or waiting for an answer: whatever sends a message, a button as much as the message
 * box, must not take an open request away. With `atOnce` it does not wait: for a command
 * Swarmed does itself. With `now` the turn under way is stopped and the message goes ahead of
 * what waits. Rejects if the message could not be handed over.
 */
export function sendOrQueue(
    id: number,
    text: string,
    files: string[] = [],
    atOnce = false,
    now = false,
): Promise<void> {
    const status = findSession(useStore.getState().projects, id)?.status;
    return delivered(api, status, id, text, files, atOnce, now);
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

/**
 * Moves the row of the sidebar that has the keyboard `by` places among its siblings, or the
 * session on screen when no row has it. A row keeps the keyboard as it moves.
 */
export async function moveFocusedRow(by: number) {
    const had = document.activeElement;
    const focused = had?.closest(".sidebar [data-order]");
    const { selectedSessionId } = useStore.getState();
    const onScreen: OrderTarget | null =
        selectedSessionId === null ? null : { kind: "session", id: selectedSessionId };
    const target = focused ? targetOf(focused) : onScreen;
    if (!target) {
        return;
    }
    await moveRow(target, by);
    if (focused) {
        // Moving the row's element on the page takes the keyboard from it. It goes back to
        // what had it, which for a task is a button of its row: its heading cannot take it.
        requestAnimationFrame(() => {
            const handle = rowOf(target)?.querySelector<HTMLElement>("[data-drag-handle]");
            (had instanceof HTMLElement && had.isConnected ? had : handle)?.focus();
        });
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

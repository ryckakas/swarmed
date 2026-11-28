import { api } from "./api";
import type { MarkColour } from "./api/types";
import { type HighlightTarget, highlightable, highlightTarget } from "./highlights";
import {
    moveTarget,
    type OrderTarget,
    type PinTarget,
    pinnable,
    pinnedNow,
    pinTarget,
    targetOf,
} from "./ordering";
import { draftOf, messageSent, withEdited } from "./rewind";
import { type Attached, delivered, githubRead, NO_DRAFT, NOTHING_ATTACHED } from "./sending";
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
export async function sendOrQueue(
    id: number,
    text: string,
    attached: Attached = NOTHING_ATTACHED,
    atOnce = false,
    now = false,
): Promise<void> {
    // The pull requests and issues it names are read first, so that sending waits on nothing.
    await githubRead(api, placeOf(id).home?.project.name, attached.github);
    // Looked at only now: its agent may have begun a turn while they were read, and a message
    // sent straight into it would take an open request away.
    const { session, home } = placeOf(id);
    // While its task is set up, a session waits as a busy one does: what is typed goes after.
    return delivered(api, standing(session?.status, home?.task), id, text, attached, atOnce, now);
}

/** Session `id` and where it belongs, as the store has them now. */
function placeOf(id: number) {
    const { projects } = useStore.getState();
    const session = findSession(projects, id);
    return { session, home: session && findSessionHome(projects, session) };
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
 * Has session `id` work for session `parent`, or with `null` for nobody: the sidebar lists it
 * under that one, and both agents hear of it with their next message.
 */
export async function setParent(id: number, parent: number | null) {
    const { refresh, report } = useStore.getState();
    try {
        await api.sessionSetParent(id, parent);
        await refresh(api);
    } catch (error) {
        report(error);
    }
}

/**
 * Gives session `id` the role called `role`, or with `null` takes its role off, and reads the
 * tree again. Its agent hears of it with the next message.
 */
export async function setRole(id: number, role: string | null) {
    const { refresh, report } = useStore.getState();
    try {
        await api.sessionSetRole(id, role);
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

/** Highlights a task or a session in `colour` where it is, or takes it off, then reads the tree again. */
export async function highlightRow(target: HighlightTarget, colour: MarkColour | null) {
    const { refresh, report } = useStore.getState();
    try {
        await highlightTarget(api, target, colour);
        await refresh(api);
    } catch (error) {
        report(error);
    }
}

/**
 * Opens the highlight palette of the task or session row that has the keyboard, or of the
 * session on screen when no row has it. A repository or a cloud task has none.
 */
export function highlightFocusedRow() {
    const target = rowInHand()?.target;
    if (target && highlightable(target)) {
        useStore.getState().askToHighlight(target);
    }
}

/**
 * Opens the snooze dialog for the session row that has the keyboard, or for the session on
 * screen when no row has it. A repository, a task or a cloud task has none.
 */
export function snoozeFocusedRow() {
    const target = rowInHand()?.target;
    if (target?.kind === "session") {
        useStore.getState().askToSnooze(target.id);
    }
}

/**
 * Snoozes session `id` until `until`, in seconds since 1970, or with `null` wakes it, then reads
 * the tree again. What went wrong is said.
 */
export async function snoozeSession(id: number, until: number | null) {
    const { refresh, report } = useStore.getState();
    try {
        await api.sessionSnooze(id, until);
        await refresh(api);
    } catch (error) {
        report(error);
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

/**
 * Asks whether to take session `id` back to before the message shown for its event `key`, and
 * how: the conversation alone, or its files too.
 */
export function editFromMessage(id: number, key: number) {
    const { events, askToRewind } = useStore.getState();
    askToRewind({ sessionId: id, message: messageNumber(events[id] ?? [], key) });
}

/**
 * Takes session `id` back to before its message number `message`, its files too with `files`,
 * and puts that message into the session's message box to be edited and sent again: never
 * sent by itself. Rejects with the core's reason if it was refused.
 */
export async function rewindSession(id: number, message: number, files: boolean) {
    const sent = messageSent(useStore.getState().events[id] ?? [], message);
    await api.sessionRewind(id, message, files);
    const { drafts, editDraft } = useStore.getState();
    if (sent !== null) {
        editDraft(id, withEdited(drafts[id] ?? NO_DRAFT, draftOf(sent)));
    }
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

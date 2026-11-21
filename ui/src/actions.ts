import { api } from "./api";
import { delivered } from "./sending";
import { useStore } from "./store";
import { messageNumber } from "./transcript";
import { findSession } from "./tree";

// Things the user can ask for from more than one place: a button, and a keyboard shortcut.

/**
 * Sends a message to a session, or has it wait in the session's queue while its agent is at
 * work or waiting for an answer: whatever sends a message, a button as much as the message
 * box, must not take an open request away. With `atOnce` it does not wait: for a command
 * Swarmed does itself. Rejects if the message could not be handed over.
 */
export function sendOrQueue(
    id: number,
    text: string,
    files: string[] = [],
    atOnce = false,
): Promise<void> {
    const status = findSession(useStore.getState().projects, id)?.status;
    return delivered(api, status, id, text, files, atOnce);
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

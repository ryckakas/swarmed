import { api } from "./api";
import { useStore } from "./store";

// Things the user can ask for from more than one place: a button, and a keyboard shortcut.

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
 * Starts a new session from what was said in another, up to its event `upto` if given, and
 * opens it.
 */
export async function forkSession(id: number, upto: number | null) {
    const { refresh, open, report } = useStore.getState();
    try {
        const fork = await api.sessionFork(id, upto);
        await refresh(api);
        await open(api, fork.id);
    } catch (error) {
        report(error);
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

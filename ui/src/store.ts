import { create } from "zustand";
import type { Api } from "./api/client";
import type {
    AgentEvent,
    NoteTarget,
    Project,
    QueuedMessage,
    RemoveTarget,
    SessionEvent,
    SessionSettings,
    Settings,
} from "./api/types";
import {
    applySessionEvent,
    findSession,
    mayHaveEdited,
    taskIdOf,
    toggledId,
    withoutSession,
    withRead,
    withSession,
} from "./tree";

interface State {
    projects: Project[];
    /** `loading` until the first read finished; an error message if it failed. */
    load: "loading" | "ready" | { error: string };
    selectedSessionId: number | null;
    /** The repository preselected on the new-task screen, shown when no session is selected. */
    draftProject: string | null;
    /** The events of every session whose transcript has been opened. */
    events: Record<number, AgentEvent[]>;
    /** Per task id, a count that goes up whenever its agents may have changed files. */
    edits: Record<number, number>;
    /** The latest line a hook printed while a task is being set up or removed. */
    hookLine: string | null;
    /** The last thing that went wrong when acting on a session, shown until dismissed. */
    problem: string | null;
    refresh(api: Api): Promise<void>;
    /** Selects a session and loads its transcript if it has not been opened yet. */
    open(api: Api, sessionId: number): Promise<void>;
    /** Shows the new-task screen, for `project` if one is given. */
    startNew(project?: string): void;
    apply(event: SessionEvent): void;
    /** Changes a session's model and supervision. */
    configure(api: Api, sessionId: number, settings: SessionSettings): Promise<void>;
    /** The user's preferences; `null` until they are read. */
    settings: Settings | null;
    /** Whether the task panel on the right is shown. */
    panelOpen: boolean;
    togglePanel(): void;
    /** Changes some of the user's preferences and keeps the rest. */
    changeSettings(api: Api, change: Partial<Settings>): Promise<void>;
    /** The ids of the repositories whose tasks are folded away in the sidebar. */
    folded: number[];
    toggleFolded(projectId: number): void;
    /** Whether the status line under the message box is shown, or folded away. */
    statusOpen: boolean;
    toggleStatus(): void;
    /** Whether the terminal is shown in place of the message box. */
    terminalOpen: boolean;
    toggleTerminal(): void;
    /** Goes up each time the user asks to rename the open session, from wherever. */
    renameAsked: number;
    askToRename(): void;
    /** Whether the settings are on screen. */
    settingsOpen: boolean;
    showSettings(open: boolean): void;
    /** Gives a session a new title. The tree follows when the app confirms it. */
    rename(api: Api, sessionId: number, title: string): Promise<void>;
    /** The repository or task the user asked to remove, until they confirm or think again. */
    removing: RemoveTarget | null;
    askToRemove(target: RemoveTarget | null): void;
    /** Notes that the user has looked at a session, here and in the core. */
    markRead(api: Api, sessionId: number): void;
    /** Goes up whenever the agents or their models change, so menus read them again. */
    agentsVersion: number;
    agentsChanged(): void;
    /** Per session, the messages waiting for its agent to finish. */
    queues: Record<number, QueuedMessage[]>;
    setQueue(sessionId: number, messages: QueuedMessage[]): void;
    /** The repository or task whose note is being edited. */
    noting: NoteTarget | null;
    /** Opens a note; `null` puts it away. */
    askForNote(target: NoteTarget | null): void;
    /** The session the user asked to close, until they confirm or change their mind. */
    closing: number | null;
    /** Asks before closing: `null` withdraws the question. */
    askToClose(sessionId: number | null): void;
    /** Stops a session and takes it out of the tree. */
    close(api: Api, sessionId: number): Promise<void>;
    report(problem: unknown): void;
    dismissProblem(): void;
}

/** Sessions whose history is being read, and whether an event arrived for them meanwhile. */
const loading = new Map<number, { missedEvent: boolean }>();

/** Reads a session's history, again if an event arrived while reading, so none is lost. */
async function readHistory(api: Api, sessionId: number): Promise<AgentEvent[]> {
    const state = { missedEvent: false };
    loading.set(sessionId, state);
    try {
        let events = await api.sessionEvents(sessionId);
        while (state.missedEvent) {
            state.missedEvent = false;
            events = await api.sessionEvents(sessionId);
        }
        return events;
    } finally {
        loading.delete(sessionId);
    }
}

/** A session's events with one more, if its transcript is open. */
export function appendEvent(
    events: Record<number, AgentEvent[]>,
    published: SessionEvent,
): Record<number, AgentEvent[]> {
    const open = events[published.session_id];
    return open ? { ...events, [published.session_id]: withEvent(open, published.event) } : events;
}

/**
 * `events` with one more. The pieces of a reply that is still being written are kept as one
 * growing event at the end, which whatever comes next replaces: the whole reply follows them.
 */
export function withEvent(events: AgentEvent[], event: AgentEvent): AgentEvent[] {
    const last = events.at(-1);
    if (last?.type !== "text_delta") {
        return [...events, event];
    }
    const next: AgentEvent =
        event.type === "text_delta" ? { ...event, text: last.text + event.text } : event;
    return [...events.slice(0, -1), next];
}

/** The edit counts after an event: one more for the task of a session that may have edited. */
function countEdit(state: State, published: SessionEvent): Record<number, number> {
    const taskId = taskIdOf(state.projects, published.session_id);
    if (taskId === undefined || !mayHaveEdited(published.event)) {
        return state.edits;
    }
    return { ...state.edits, [taskId]: (state.edits[taskId] ?? 0) + 1 };
}

const PANEL_OPEN = "swarmed.panel-open";
const STATUS_OPEN = "swarmed.status-open";
const FOLDED = "swarmed.folded-projects";

/** A list of ids remembered on this machine; empty if nothing usable is stored. */
function readIds(key: string): number[] {
    try {
        const stored: unknown = JSON.parse(localStorage.getItem(key) ?? "[]");
        return Array.isArray(stored) ? stored.filter((id) => typeof id === "number") : [];
    } catch {
        return [];
    }
}

function writeStored(key: string, value: string) {
    try {
        localStorage.setItem(key, value);
    } catch {
        // Without storage the choice just lasts until the window closes.
    }
}

/** A yes or no remembered on this machine; `fallback` if nothing, or nothing readable, is stored. */
function readFlag(key: string, fallback: boolean): boolean {
    try {
        const stored = localStorage.getItem(key);
        return stored === null ? fallback : stored === "true";
    } catch {
        return fallback;
    }
}

function writeFlag(key: string, value: boolean) {
    try {
        localStorage.setItem(key, String(value));
    } catch {
        // Without storage the choice just lasts until the window closes.
    }
}

/** Runs an action on a session; if it fails, the reason is shown instead of thrown. */
async function reporting(action: () => Promise<void>) {
    try {
        await action();
    } catch (error) {
        useStore.getState().report(error);
    }
}

/** What changes when a session is closed: it leaves the tree, and the centre if it was there. */
function closed(state: State, sessionId: number): Partial<State> {
    const selected = state.selectedSessionId;
    return {
        closing: null,
        projects: withoutSession(state.projects, sessionId),
        selectedSessionId: selected === sessionId ? null : selected,
    };
}

export const useStore = create<State>((set, get) => ({
    projects: [],
    load: "loading",
    selectedSessionId: null,
    draftProject: null,
    events: {},
    edits: {},
    hookLine: null,
    problem: null,
    settings: null,
    panelOpen: readFlag(PANEL_OPEN, true),
    togglePanel: () => {
        const panelOpen = !get().panelOpen;
        writeFlag(PANEL_OPEN, panelOpen);
        set({ panelOpen });
    },
    changeSettings: (api, change) =>
        reporting(async () => {
            const current = get().settings ?? (await api.settings());
            const settings = { ...current, ...change };
            // Shown at once; the app only has to remember it.
            set({ settings });
            await api.setSettings(settings);
        }),
    folded: readIds(FOLDED),
    toggleFolded: (projectId) => {
        const folded = toggledId(get().folded, projectId);
        writeStored(FOLDED, JSON.stringify(folded));
        set({ folded });
    },
    statusOpen: readFlag(STATUS_OPEN, true),
    toggleStatus: () => {
        const statusOpen = !get().statusOpen;
        writeFlag(STATUS_OPEN, statusOpen);
        set({ statusOpen });
    },
    terminalOpen: false,
    toggleTerminal: () => set((state) => ({ terminalOpen: !state.terminalOpen })),
    renameAsked: 0,
    askToRename: () => set((state) => ({ renameAsked: state.renameAsked + 1 })),
    settingsOpen: false,
    showSettings: (open) => set({ settingsOpen: open }),
    rename: (api, sessionId, title) =>
        reporting(async () => {
            await api.sessionRename(sessionId, title);
        }),
    removing: null,
    askToRemove: (target) => set({ removing: target }),
    markRead: (api, sessionId) => {
        if (!findSession(get().projects, sessionId)?.unread) {
            return;
        }
        set((state) => ({ projects: withRead(state.projects, sessionId) }));
        api.sessionMarkRead(sessionId).catch(get().report);
    },
    agentsVersion: 0,
    agentsChanged: () => set((state) => ({ agentsVersion: state.agentsVersion + 1 })),
    queues: {},
    setQueue: (sessionId, messages) =>
        set((state) => ({ queues: { ...state.queues, [sessionId]: messages } })),
    noting: null,
    askForNote: (target) => set({ noting: target }),
    closing: null,
    askToClose: (sessionId) => set({ closing: sessionId }),
    refresh: async (api) => {
        try {
            const projects = await api.overview();
            // A session that is gone, with its task or repository, cannot stay in the centre.
            const selected = findSession(projects, get().selectedSessionId)?.id ?? null;
            set({ projects, load: "ready", selectedSessionId: selected });
        } catch (error) {
            set({ load: { error: String(error) } });
        }
    },
    open: async (api, sessionId) => {
        set({ selectedSessionId: sessionId });
        get().markRead(api, sessionId);
        if (get().events[sessionId] || loading.has(sessionId)) {
            return;
        }
        try {
            const history = await readHistory(api, sessionId);
            set((state) => ({ events: { ...state.events, [sessionId]: history } }));
        } catch (error) {
            get().report(error);
        }
    },
    startNew: (project) => set({ selectedSessionId: null, draftProject: project ?? null }),
    apply: (published) => {
        // A queue is not something that was said: it is kept beside the transcript, not in it.
        if (published.event.type === "queue") {
            return get().setQueue(published.session_id, published.event.messages);
        }
        const reading = loading.get(published.session_id);
        if (reading) {
            reading.missedEvent = true;
        }
        set((state) => ({
            projects: applySessionEvent(state.projects, published),
            events: appendEvent(state.events, published),
            edits: countEdit(state, published),
        }));
    },
    configure: (api, sessionId, settings) =>
        reporting(async () => {
            const session = await api.sessionConfigure(sessionId, settings);
            set((state) => ({ projects: withSession(state.projects, session) }));
        }),
    close: (api, sessionId) =>
        reporting(async () => {
            await api.sessionClose(sessionId);
            set((state) => closed(state, sessionId));
        }),
    report: (problem) => set({ problem: String(problem) }),
    dismissProblem: () => set({ problem: null }),
}));

/** The window the app is shown in. There is none where the store runs without one, as in tests. */
const inView = typeof window === "undefined" ? null : window;

/** Loads the tree and keeps it in step with the sessions. Returns what stops that again. */
export function connect(api: Api): () => void {
    const { refresh, apply } = useStore.getState();
    void refresh(api);
    void reporting(async () => useStore.setState({ settings: await api.settings() }));
    // After dropped events nothing on screen can be trusted: forget the transcripts and reload.
    const reload = () => {
        const { selectedSessionId, open } = useStore.getState();
        useStore.setState({ events: {} });
        void refresh(api);
        if (selectedSessionId !== null) {
            void open(api, selectedSessionId);
        }
    };
    // Fresh closures on every call: React mounts twice in development, and an `Api` that tells
    // listeners apart by identity would let the first mount's cleanup remove the second's.
    // An answer in the session on screen is read as it arrives, if the window is being looked
    // at; otherwise when the window is.
    const readIfSeen = () => {
        const { selectedSessionId, markRead } = useStore.getState();
        if (selectedSessionId !== null && inView?.document.hasFocus()) {
            markRead(api, selectedSessionId);
        }
    };
    inView?.addEventListener("focus", readIfSeen);
    const subscriptions = [
        api.onSessionEvent((event) => {
            // A session the tree does not know was started by an agent, not from here: the
            // tree is read again so that it appears.
            if (!findSession(useStore.getState().projects, event.session_id)) {
                void refresh(api);
            }
            apply(event);
            readIfSeen();
        }),
        api.onEventsLagged(() => reload()),
        api.onHookOutput((output) => useStore.setState({ hookLine: output.line })),
    ];
    return () => {
        inView?.removeEventListener("focus", readIfSeen);
        for (const subscription of subscriptions) {
            void subscription.then((unsubscribe) => unsubscribe());
        }
    };
}

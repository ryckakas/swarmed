import { create } from "zustand";
import type { Api } from "./api/client";
import type { AgentEvent, Project, SessionEvent } from "./api/types";
import { applySessionEvent } from "./tree";

interface State {
    projects: Project[];
    /** `loading` until the first read finished; an error message if it failed. */
    load: "loading" | "ready" | { error: string };
    selectedSessionId: number | null;
    /** The repository preselected on the new-task screen, shown when no session is selected. */
    draftProject: string | null;
    /** The events of every session whose transcript has been opened. */
    events: Record<number, AgentEvent[]>;
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
    return open ? { ...events, [published.session_id]: [...open, published.event] } : events;
}

export const useStore = create<State>((set, get) => ({
    projects: [],
    load: "loading",
    selectedSessionId: null,
    draftProject: null,
    events: {},
    hookLine: null,
    problem: null,
    refresh: async (api) => {
        try {
            set({ projects: await api.overview(), load: "ready" });
        } catch (error) {
            set({ load: { error: String(error) } });
        }
    },
    open: async (api, sessionId) => {
        set({ selectedSessionId: sessionId });
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
        const reading = loading.get(published.session_id);
        if (reading) {
            reading.missedEvent = true;
        }
        set((state) => ({
            projects: applySessionEvent(state.projects, published),
            events: appendEvent(state.events, published),
        }));
    },
    report: (problem) => set({ problem: String(problem) }),
    dismissProblem: () => set({ problem: null }),
}));

/** Loads the tree and keeps it in step with the sessions. Returns what stops that again. */
export function connect(api: Api): () => void {
    const { refresh, apply } = useStore.getState();
    void refresh(api);
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
    const subscriptions = [
        api.onSessionEvent((event) => apply(event)),
        api.onEventsLagged(() => reload()),
        api.onHookOutput((output) => useStore.setState({ hookLine: output.line })),
    ];
    return () => {
        for (const subscription of subscriptions) {
            void subscription.then((unsubscribe) => unsubscribe());
        }
    };
}

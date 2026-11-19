import { create } from "zustand";
import type { Api } from "./api/client";
import type { AgentEvent, Project, SessionEvent, SessionSettings } from "./api/types";
import { applySessionEvent, withoutSession, withSession } from "./tree";

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
    /** Changes a session's model and supervision. */
    configure(api: Api, sessionId: number, settings: SessionSettings): Promise<void>;
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
    hookLine: null,
    problem: null,
    closing: null,
    askToClose: (sessionId) => set({ closing: sessionId }),
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

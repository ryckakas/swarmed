import { create } from "zustand";
import type { Api } from "./api/client";
import type { Project, SessionEvent } from "./api/types";
import { applySessionEvent } from "./tree";

interface State {
    projects: Project[];
    /** `loading` until the first read finished; an error message if it failed. */
    load: "loading" | "ready" | { error: string };
    selectedSessionId: number | null;
    refresh(api: Api): Promise<void>;
    select(sessionId: number): void;
    apply(event: SessionEvent): void;
}

export const useStore = create<State>((set) => ({
    projects: [],
    load: "loading",
    selectedSessionId: null,
    refresh: async (api) => {
        try {
            set({ projects: await api.overview(), load: "ready" });
        } catch (error) {
            set({ load: { error: String(error) } });
        }
    },
    select: (sessionId) => set({ selectedSessionId: sessionId }),
    apply: (event) => set((state) => ({ projects: applySessionEvent(state.projects, event) })),
}));

/** Loads the tree and keeps it in step with the sessions. Returns what stops that again. */
export function connect(api: Api): () => void {
    const { refresh, apply } = useStore.getState();
    void refresh(api);
    const subscriptions = [api.onSessionEvent(apply), api.onEventsLagged(() => void refresh(api))];
    return () => {
        for (const subscription of subscriptions) {
            void subscription.then((unsubscribe) => unsubscribe());
        }
    };
}

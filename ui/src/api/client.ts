import type { Project, SessionEvent } from "./types";

/** Stops a subscription. */
export type Unsubscribe = () => void;

/** Everything the UI asks of the app. The Tauri shell implements it; so does the mock. */
export interface Api {
    overview(): Promise<Project[]>;
    /** Calls `listener` for every event of every running session, from now on. */
    onSessionEvent(listener: (event: SessionEvent) => void): Promise<Unsubscribe>;
    /** Calls `listener` when events were dropped and anything on screen should be reloaded. */
    onEventsLagged(listener: () => void): Promise<Unsubscribe>;
}

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Api } from "./client";
import type { Project, SessionEvent } from "./types";

/** The real app: commands and events of the Tauri shell in crates/app. */
export const tauriApi: Api = {
    overview: () => invoke<Project[]>("overview"),
    onSessionEvent: (listener) =>
        listen<SessionEvent>("session-event", (event) => listener(event.payload)),
    onEventsLagged: (listener) => listen("session-events-lagged", () => listener()),
};

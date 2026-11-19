import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type { Api } from "./client";
import type { AgentEvent, HookOutput, Project, Session, SessionEvent, Task } from "./types";

/** The real app: commands and events of the Tauri shell in crates/app. */
export const tauriApi: Api = {
    overview: () => invoke<Project[]>("overview"),
    pickFolder: () => open({ directory: true, multiple: false, title: "Add a repository" }),
    projectAdd: (path) => invoke<Project>("project_add", { path, name: null }),
    taskNew: (project, name) => invoke<Task>("task_new", { project, name, branch: null }),
    onHookOutput: (listener) =>
        listen<HookOutput>("hook-output", (event) => listener(event.payload)),
    sessionNew: (project, task) =>
        invoke<Session>("session_new", { project, task, model: null, supervision: "ask" }),
    sessionEvents: (id) => invoke<AgentEvent[]>("session_events", { id }),
    sessionSend: (id, text) => invoke("session_send", { id, text }),
    sessionAllow: (id, requestId) => invoke("session_allow", { id, requestId }),
    sessionDeny: (id, requestId, reason) => invoke("session_deny", { id, requestId, reason }),
    sessionInterrupt: (id) => invoke("session_interrupt", { id }),
    onSessionEvent: (listener) =>
        listen<SessionEvent>("session-event", (event) => listener(event.payload)),
    onEventsLagged: (listener) => listen("session-events-lagged", () => listener()),
};

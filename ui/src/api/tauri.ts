import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type { Api } from "./client";
import type {
    AgentEvent,
    AgentInfo,
    ApiKeyInfo,
    BranchChoices,
    FileChange,
    FileDiff,
    HookOutput,
    Project,
    Session,
    SessionEvent,
    Settings,
    Task,
} from "./types";

/** The real app: commands and events of the Tauri shell in crates/app. */
export const tauriApi: Api = {
    overview: () => invoke<Project[]>("overview"),
    agents: () => invoke<AgentInfo[]>("agents"),
    pickFolder: () => open({ directory: true, multiple: false, title: "Add a repository" }),
    projectAdd: (path) => invoke<Project>("project_add", { path, name: null }),
    branchChoices: (project) => invoke<BranchChoices>("branch_choices", { project }),
    taskNew: (project, name, branch) => invoke<Task>("task_new", { project, name, branch }),
    taskChanges: (project, name) => invoke<FileChange[]>("task_changes", { project, name }),
    taskDiff: (project, name, path) => invoke<FileDiff>("task_diff", { project, name, path }),
    taskOpenEditor: (project, name) => invoke("task_open_editor", { project, name }),
    taskOpenUrl: (project, name) => invoke("task_open_url", { project, name }),
    apiKeys: () => invoke<ApiKeyInfo[]>("api_keys"),
    setApiKey: (name, key) => invoke("set_api_key", { name, key }),
    deleteApiKey: (name) => invoke("delete_api_key", { name }),
    settings: () => invoke<Settings>("settings"),
    setSettings: (settings) => invoke("set_settings", { settings }),
    onHookOutput: (listener) =>
        listen<HookOutput>("hook-output", (event) => listener(event.payload)),
    sessionNew: (project, task, setup) =>
        invoke<Session>("session_new", { project, task, ...setup }),
    sessionEvents: (id) => invoke<AgentEvent[]>("session_events", { id }),
    sessionSend: (id, text) => invoke("session_send", { id, text }),
    sessionAllow: (id, requestId) => invoke("session_allow", { id, requestId }),
    sessionDeny: (id, requestId, reason) => invoke("session_deny", { id, requestId, reason }),
    sessionInterrupt: (id) => invoke("session_interrupt", { id }),
    slashCommands: (id) => invoke<string[]>("slash_commands", { id }),
    sessionConfigure: (id, settings) => invoke<Session>("session_configure", { id, ...settings }),
    sessionRename: (id, title) => invoke<Session>("session_rename", { id, title }),
    sessionClose: (id) => invoke("session_close", { id }),
    onSessionEvent: (listener) =>
        listen<SessionEvent>("session-event", (event) => listener(event.payload)),
    onEventsLagged: (listener) => listen("session-events-lagged", () => listener()),
};

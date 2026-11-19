import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type { Api } from "./client";
import type {
    AgentEvent,
    AgentInfo,
    AgentSetup,
    ApiKeyInfo,
    BranchChoices,
    FileChange,
    FileDiff,
    HookOutput,
    McpServerView,
    Project,
    QueuedMessage,
    Session,
    SessionEvent,
    Settings,
    Task,
    TaskNaming,
    TerminalOutput,
    Usage,
} from "./types";

/** The real app: commands and events of the Tauri shell in crates/app. */
export const tauriApi: Api = {
    overview: () => invoke<Project[]>("overview"),
    agents: () => invoke<AgentInfo[]>("agents"),
    mcpServers: () => invoke<McpServerView[]>("mcp_servers"),
    mcpSave: (server) => invoke<McpServerView[]>("mcp_save", { server }),
    mcpRemove: (name) => invoke<McpServerView[]>("mcp_remove", { name }),
    usage: () => invoke<Usage>("usage"),
    agentSetups: () => invoke<AgentSetup[]>("agent_setups"),
    agentInstall: (id) => invoke<AgentSetup>("agent_install", { id }),
    agentConfigure: (id, models) => invoke<AgentSetup>("agent_configure", { id, models }),
    openrouterModels: () => invoke<string[]>("openrouter_models"),
    pickFolder: () => open({ directory: true, multiple: false, title: "Add a repository" }),
    projectAdd: (path) => invoke<Project>("project_add", { path, name: null }),
    projectRemove: (name, force) => invoke("project_remove", { name, force }),
    taskRemove: (project, name, force) =>
        invoke("task_remove", { project, name, force, deleteBranch: false }),
    branchChoices: (project) => invoke<BranchChoices>("branch_choices", { project }),
    taskNameFor: (project, prompt) =>
        invoke<TaskNaming | null>("task_name_for", { project, prompt }),
    taskNew: (project, name, branch) => invoke<Task>("task_new", { project, name, branch }),
    taskChanges: (project, name) => invoke<FileChange[]>("task_changes", { project, name }),
    taskDiff: (project, name, path) => invoke<FileDiff>("task_diff", { project, name, path }),
    taskOpenEditor: (project, name) => invoke("task_open_editor", { project, name }),
    taskOpenUrl: (project, name) => invoke("task_open_url", { project, name }),
    note: (project, task) => invoke<string>("note", { project, task }),
    setNote: (project, task, text) => invoke("set_note", { project, task, text }),
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
    sessionQueue: (id) => invoke<QueuedMessage[]>("session_queue", { id }),
    sessionEnqueue: (id, text) => invoke<QueuedMessage[]>("session_enqueue", { id, text }),
    sessionQueueEdit: (id, message, text) =>
        invoke<QueuedMessage[]>("session_queue_edit", { id, message, text }),
    sessionSendQueued: (id) => invoke("session_send_queued", { id }),
    sessionMarkRead: (id) => invoke("session_mark_read", { id }),
    sessionAllow: (id, requestId) => invoke("session_allow", { id, requestId }),
    sessionDeny: (id, requestId, reason) => invoke("session_deny", { id, requestId, reason }),
    sessionInterrupt: (id) => invoke("session_interrupt", { id }),
    slashCommands: (id) => invoke<string[]>("slash_commands", { id }),
    sessionConfigure: (id, settings) => invoke<Session>("session_configure", { id, ...settings }),
    viewUrl: async (document) => {
        const id = await invoke<string>("view_put", { document });
        return `swarmed-view://localhost/${id}`;
    },
    viewOpen: (document) => invoke("view_open", { document }),
    terminalOpen: (id, cols, rows) => invoke<string>("terminal_open", { id, cols, rows }),
    terminalWrite: (id, data) => invoke("terminal_write", { id, data }),
    terminalResize: (id, cols, rows) => invoke("terminal_resize", { id, cols, rows }),
    onTerminalOutput: (listener) =>
        listen<TerminalOutput>("terminal-output", (event) => listener(event.payload)),
    statusLine: (id) => invoke<string | null>("status_line", { id }),
    sessionRename: (id, title) => invoke<Session>("session_rename", { id, title }),
    sessionClose: (id) => invoke("session_close", { id }),
    sessionsClosed: (project, task) => invoke<Session[]>("sessions_closed", { project, task }),
    sessionReopen: (id) => invoke<Session>("session_reopen", { id }),
    onSessionEvent: (listener) =>
        listen<SessionEvent>("session-event", (event) => listener(event.payload)),
    onEventsLagged: (listener) => listen("session-events-lagged", () => listener()),
};

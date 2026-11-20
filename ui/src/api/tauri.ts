import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import type { Api } from "./client";
import type {
    AgentEvent,
    AgentInfo,
    AgentSetup,
    ApiKeyInfo,
    BranchChoices,
    CloudTask,
    Costs,
    Environment,
    FileChange,
    FileDiff,
    HookOutput,
    McpServerView,
    Project,
    Pull,
    PullDraft,
    QueuedMessage,
    SearchHit,
    Session,
    SessionEvent,
    Settings,
    Skill,
    Task,
    TaskNaming,
    Template,
    TerminalOutput,
    Unpublished,
    Usage,
} from "./types";

/** The real app: commands and events of the Tauri shell in crates/app. */
export const tauriApi: Api = {
    overview: () => invoke<Project[]>("overview"),
    agents: () => invoke<AgentInfo[]>("agents"),
    mcpServers: () => invoke<McpServerView[]>("mcp_servers"),
    mcpSave: (server) => invoke<McpServerView[]>("mcp_save", { server }),
    mcpRemove: (name) => invoke<McpServerView[]>("mcp_remove", { name }),
    templates: () => invoke<Template[]>("templates"),
    templateSave: (name, text) => invoke<Template[]>("template_save", { name, text }),
    templateRemove: (name) => invoke<Template[]>("template_remove", { name }),
    usage: () => invoke<Usage>("usage"),
    costs: () => invoke<Costs>("costs"),
    snapshots: (project) => invoke("snapshots", { project }),
    snapshotSave: (project, task) => invoke("snapshot_save", { project, task }),
    snapshotRemove: (project, environment) => invoke("snapshot_remove", { project, environment }),
    analytics: () => invoke("analytics"),
    effortDefault: (agent, model) => invoke<string | null>("effort_default", { agent, model }),
    agentSetups: () => invoke<AgentSetup[]>("agent_setups"),
    agentInstall: (id) => invoke<AgentSetup>("agent_install", { id }),
    agentConfigure: (id, models) => invoke<AgentSetup>("agent_configure", { id, models }),
    agentModelsRefresh: (id) => invoke<AgentSetup>("agent_models_refresh", { id }),
    openrouterModels: () => invoke<string[]>("openrouter_models"),
    pickFiles: async () => {
        const chosen = await open({ multiple: true, title: "Point the agent at files" });
        return chosen ?? [];
    },
    onFilesDropped: (listener) =>
        getCurrentWebview().onDragDropEvent((event) => {
            if (event.payload.type === "drop") {
                listener(event.payload.paths);
            }
        }),
    pickFolder: () => open({ directory: true, multiple: false, title: "Add a repository" }),
    projectAdd: (path) => invoke<Project>("project_add", { path, name: null }),
    projectRemove: (name, force) => invoke("project_remove", { name, force }),
    taskRemove: (project, name, force) =>
        invoke("task_remove", { project, name, force, deleteBranch: false }),
    branchChoices: (project) => invoke<BranchChoices>("branch_choices", { project }),
    taskNameFor: (project, prompt) =>
        invoke<TaskNaming | null>("task_name_for", { project, prompt }),
    taskNew: (project, name, branch, environment) =>
        invoke<Task>("task_new", { project, name, branch, environment }),
    cloudDelegate: (project, prompt, task, provider) =>
        invoke<CloudTask>("cloud_delegate", { project, prompt, task, provider }),
    cloudTasks: (project, refresh) => invoke<CloudTask[]>("cloud_tasks", { project, refresh }),
    cloudTaskForget: (id) => invoke<void>("cloud_task_forget", { id }),
    cloudTaskOpen: (id) => invoke<void>("cloud_task_open", { id }),
    cloudTaskDiff: (id) => invoke<string>("cloud_task_diff", { id }),
    cloudTaskApply: (id, project, name) => invoke<void>("cloud_task_apply", { id, project, name }),
    stackStart: (project, task) => invoke<boolean>("stack_start", { project, task }),
    stackStop: (project, task) => invoke<void>("stack_stop", { project, task }),
    stackLog: (project, task) => invoke<string>("stack_log", { project, task }),
    skills: () => invoke<Skill[]>("skills"),
    skillBody: (name) => invoke<string>("skill_body", { name }),
    skillSave: (name, description, body) =>
        invoke<Skill[]>("skill_save", { name, description, body }),
    skillRemove: (name) => invoke<Skill[]>("skill_remove", { name }),
    skillShare: (origin, name, shared) => invoke<Skill[]>("skill_share", { origin, name, shared }),
    environments: (project) => invoke<Record<string, Environment>>("environments", { project }),
    environmentSave: (project, name, environment) =>
        invoke<void>("environment_save", { project, name, environment }),
    environmentRemove: (project, name) => invoke<void>("environment_remove", { project, name }),
    taskUnpublished: (project, name) => invoke<Unpublished>("task_unpublished", { project, name }),
    taskCommitMessage: (project, name, session) =>
        invoke<string>("task_commit_message", { project, name, session }),
    taskCommit: (project, name, message) => invoke<void>("task_commit", { project, name, message }),
    taskPush: (project, name) => invoke<void>("task_push", { project, name }),
    taskPullDraft: (project, name) => invoke<PullDraft>("task_pull_draft", { project, name }),
    taskPullCreate: (project, name, draft, asDraft) =>
        invoke<Pull>("task_pull_create", { project, name, draft, asDraft }),
    taskPull: (project, name) => invoke<Pull | null>("task_pull", { project, name }),
    taskPullOpen: (project, name, check) =>
        invoke<void>("task_pull_open", { project, name, check }),
    taskChanges: (project, name) => invoke<FileChange[]>("task_changes", { project, name }),
    taskDiff: (project, name, path) => invoke<FileDiff>("task_diff", { project, name, path }),
    taskOpenEditor: (project, name) => invoke("task_open_editor", { project, name }),
    taskOpenUrl: (project, name) => invoke("task_open_url", { project, name }),
    openLink: (url) => invoke("open_link", { url }),
    note: (project, task) => invoke<string>("note", { project, task }),
    setNote: (project, task, text) => invoke("set_note", { project, task, text }),
    apiKeys: () => invoke<ApiKeyInfo[]>("api_keys"),
    setApiKey: (name, key) => invoke("set_api_key", { name, key }),
    deleteApiKey: (name) => invoke("delete_api_key", { name }),
    settings: () => invoke<Settings>("settings"),
    setSettings: (settings) => invoke("set_settings", { settings }),
    setBadge: (count) => invoke("badge_set", { count }),
    onHookOutput: (listener) =>
        listen<HookOutput>("hook-output", (event) => listener(event.payload)),
    sessionFork: (id, upto) => invoke<Session>("session_fork", { id, upto }),
    sessionNew: (project, task, { agent, ...how }) =>
        invoke<Session>("session_new", { project, task, agent, how }),
    sessionEvents: (id) => invoke<AgentEvent[]>("session_events", { id }),
    sessionSend: (id, text, files) => invoke("session_send", { id, text, files: files ?? [] }),
    sessionQueue: (id) => invoke<QueuedMessage[]>("session_queue", { id }),
    sessionEnqueue: (id, text, files) =>
        invoke<QueuedMessage[]>("session_enqueue", { id, text, files: files ?? [] }),
    sessionQueueEdit: (id, message, text) =>
        invoke<QueuedMessage[]>("session_queue_edit", { id, message, text }),
    sessionSendQueued: (id) => invoke("session_send_queued", { id }),
    sessionMarkRead: (id) => invoke("session_mark_read", { id }),
    sessionAllow: (id, requestId) => invoke("session_allow", { id, requestId }),
    sessionDeny: (id, requestId, reason) => invoke("session_deny", { id, requestId, reason }),
    sessionInterrupt: (id) => invoke("session_interrupt", { id }),
    taskBackground: (project, name) => invoke("task_background", { project, name }),
    sessionStopBackground: (id, taskId) => invoke("session_stop_background", { id, taskId }),
    slashCommands: (id) => invoke<string[]>("slash_commands", { id }),
    sessionConfigure: (id, settings) => invoke<Session>("session_configure", { id, how: settings }),
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
    sessionsSearch: (query) => invoke<SearchHit[]>("sessions_search", { query }),
    sessionsClosed: (project, task) => invoke<Session[]>("sessions_closed", { project, task }),
    sessionReopen: (id) => invoke<Session>("session_reopen", { id }),
    onSessionEvent: (listener) =>
        listen<SessionEvent>("session-event", (event) => listener(event.payload)),
    onNoticeOpened: (listener) =>
        listen<number>("notice-opened", (event) => listener(event.payload)),
    onEventsLagged: (listener) => listen("session-events-lagged", () => listener()),
};

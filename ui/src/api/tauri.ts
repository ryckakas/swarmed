import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import type { Api } from "./client";
import type {
    AccountUsage,
    Action,
    AgentEvent,
    AgentInfo,
    AgentSetup,
    ApiKeyInfo,
    AppShown,
    BackgroundOutput,
    BranchChoices,
    BrowserEvent,
    BrowserSetup,
    BrowserState,
    BuiltinMcpView,
    Cleaned,
    Cleanup,
    CloudTask,
    ConsoleLine,
    Costs,
    Descendant,
    Environment,
    FileChange,
    FileDiff,
    GitHubItem,
    GitHubItems,
    HookOutput,
    Issue,
    IssueDetail,
    IssueList,
    McpServerView,
    MergeReport,
    MergeRules,
    PickedElement,
    Project,
    Pull,
    PullDraft,
    QueuedMessage,
    Reachable,
    RewindCheck,
    RewindDone,
    RoleInfo,
    SearchHit,
    Session,
    SessionEvent,
    Settings,
    SetupLog,
    Skill,
    SlashCommand,
    Task,
    TaskNaming,
    TaskStarted,
    Template,
    TerminalOpened,
    TerminalOutput,
    Turn,
    TurnChanges,
    Unpublished,
    UpdateState,
    Usage,
    UsageAsk,
} from "./types";

/** The real app: commands and events of the Tauri shell in crates/app. */
/** What a message points at besides its words, as the core's commands take it: none for each left out. */
function attached(files?: string[], sessions?: number[], github?: number[], elements?: number[]) {
    return {
        files: files ?? [],
        sessions: sessions ?? [],
        github: github ?? [],
        elements: elements ?? [],
    };
}

export const tauriApi: Api = {
    overview: () => invoke<Project[]>("overview"),
    agents: () => invoke<AgentInfo[]>("agents"),
    mcpServers: () => invoke<McpServerView[]>("mcp_servers"),
    mcpSave: (server) => invoke<McpServerView[]>("mcp_save", { server }),
    mcpRemove: (name) => invoke<McpServerView[]>("mcp_remove", { name }),
    builtinMcpServers: () => invoke<BuiltinMcpView[]>("builtin_mcp_servers"),
    builtinMcpCheck: (name, full) => invoke<BuiltinMcpView>("builtin_mcp_check", { name, full }),
    builtinMcpInstall: (name) => invoke<BuiltinMcpView>("builtin_mcp_install", { name }),
    builtinMcpTurnOn: (name, copy) => invoke<BuiltinMcpView>("builtin_mcp_turn_on", { name, copy }),
    builtinMcpTurnOff: (name) => invoke<BuiltinMcpView>("builtin_mcp_turn_off", { name }),
    builtinMcpRemoveRegistration: (name, key) =>
        invoke<BuiltinMcpView>("builtin_mcp_remove_registration", { name, key }),
    builtinMcpRestartSessions: (name) =>
        invoke<BuiltinMcpView>("builtin_mcp_restart_sessions", { name }),
    builtinMcpOpen: (name) => invoke<void>("builtin_mcp_open", { name }),
    builtinMcpTrashOld: (name) => invoke<BuiltinMcpView>("builtin_mcp_trash_old", { name }),
    actions: (project) => invoke<Action[]>("actions", { project }),
    actionSave: (project, action) => invoke<Action[]>("action_save", { project, action }),
    actionMove: (project, name, by) => invoke<Action[]>("action_move", { project, name, by }),
    actionRemove: (project, name) => invoke<Action[]>("action_remove", { project, name }),
    templates: () => invoke<Template[]>("templates"),
    templateSave: (name, text) => invoke<Template[]>("template_save", { name, text }),
    templateRemove: (name) => invoke<Template[]>("template_remove", { name }),
    usage: () => invoke<Usage>("usage"),
    cleanup: () => invoke<Cleanup>("cleanup"),
    cleanupClean: (kind, keys, atRisk) => invoke<Cleaned>("cleanup_clean", { kind, keys, atRisk }),
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
    projectMove: (project, by) => invoke<void>("project_move", { project, by }),
    taskMove: (project, task, by) => invoke<void>("task_move", { project, task, by }),
    sessionMove: (id, by) => invoke<void>("session_move", { id, by }),
    cloudTaskMove: (id, by) => invoke<void>("cloud_task_move", { id, by }),
    projectPin: (project, pinned) => invoke<void>("project_pin", { project, pinned }),
    taskPin: (project, task, pinned) => invoke<void>("task_pin", { project, task, pinned }),
    sessionPin: (id, pinned) => invoke<void>("session_pin", { id, pinned }),
    taskHighlight: (project, task, colour) =>
        invoke<void>("task_highlight", { project, task, colour }),
    sessionHighlight: (id, colour) => invoke<void>("session_highlight", { id, colour }),
    sessionSnooze: (id, until) => invoke<void>("session_snooze", { id, until }),
    taskRemove: (project, name, force) =>
        // Its branch goes too, unless commits on it exist nowhere else: the core keeps it then.
        invoke("task_remove", { project, name, force, deleteBranch: true }),
    taskDone: (project, name, closeWorkers) =>
        invoke<string[]>("task_done", { project, name, closeWorkers }),
    taskDescendantsElsewhere: (project, name) =>
        invoke<Descendant[]>("task_descendants_elsewhere", { project, name }),
    taskUndone: (project, name) => invoke<void>("task_undone", { project, name }),
    branchChoices: (project) => invoke<BranchChoices>("branch_choices", { project }),
    taskNameFor: (project, prompt) =>
        invoke<TaskNaming | null>("task_name_for", { project, prompt }),
    taskNew: (project, name, branch, environment) =>
        invoke<Task>("task_new", { project, name, branch, environment }),
    taskStart: (project, name, { setup: { agent, role, ...how }, ...start }) =>
        invoke<TaskStarted>("task_start", {
            project,
            name,
            start: { ...start, agent, role: role ?? null, how },
        }),
    taskSetup: (project, name) => invoke<Task>("task_setup", { project, name }),
    taskSetupStop: (project, name) => invoke<void>("task_setup_stop", { project, name }),
    taskStartAnyway: (project, name) => invoke<number[]>("task_start_anyway", { project, name }),
    taskSetupLog: (project, name) => invoke<SetupLog>("task_setup_log", { project, name }),
    cloudDelegate: (project, prompt, task, provider) =>
        invoke<CloudTask>("cloud_delegate", { project, prompt, task, provider }),
    cloudDelegateConversation: (id, instruction, provider) =>
        invoke<CloudTask>("cloud_delegate_conversation", { id, instruction, provider }),
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
    environmentDefault: (project) => invoke<string | null>("environment_default", { project }),
    environmentSetDefault: (project, name) =>
        invoke<void>("environment_set_default", { project, name }),
    taskUnpublished: (project, name) => invoke<Unpublished>("task_unpublished", { project, name }),
    taskPullMessage: (project, name, session) =>
        invoke<PullDraft>("task_pull_message", { project, name, session }),
    taskCommitMessage: (project, name, session) =>
        invoke<string>("task_commit_message", { project, name, session }),
    taskCommit: (project, name, message) => invoke<void>("task_commit", { project, name, message }),
    taskPush: (project, name) => invoke<void>("task_push", { project, name }),
    taskSwitch: (project, name, branch) => invoke<void>("task_switch", { project, name, branch }),
    taskPullDraft: (project, name) => invoke<PullDraft>("task_pull_draft", { project, name }),
    taskPullCreate: (project, name, draft, asDraft) =>
        invoke<Pull>("task_pull_create", { project, name, draft, asDraft }),
    taskPull: (project, name) => invoke<Pull | null>("task_pull", { project, name }),
    taskPullFailure: (project, name) => invoke<string>("task_pull_failure", { project, name }),
    taskPullOpen: (project, name, check) =>
        invoke<void>("task_pull_open", { project, name, check }),
    projectIssues: (project, filter) => invoke<IssueList>("project_issues", { project, filter }),
    projectIssue: (project, issue) =>
        invoke<IssueDetail>("project_issue", { project, number: issue }),
    taskIssue: (project, name) => invoke<Issue | null>("task_issue", { project, name }),
    projectMergeRules: (project) => invoke<MergeRules>("project_merge_rules", { project }),
    taskPullMerge: (project, name, method, when, head) =>
        invoke<MergeReport>("task_pull_merge", { project, name, method, when, head }),
    taskPullAutoMergeOff: (project, name) =>
        invoke<MergeReport>("task_pull_auto_merge_off", { project, name }),
    taskChanges: (project, name) => invoke<FileChange[]>("task_changes", { project, name }),
    taskDiff: (project, name, path) => invoke<FileDiff>("task_diff", { project, name, path }),
    sessionTurns: (id) => invoke<Turn[]>("session_turns", { id }),
    sessionTurnChanges: (id, message) =>
        invoke<TurnChanges>("session_turn_changes", { id, message }),
    sessionTurnDiff: (id, message, path) =>
        invoke<FileDiff>("session_turn_diff", { id, message, path }),
    sessionRewindCheck: (id, message) =>
        invoke<RewindCheck>("session_rewind_check", { id, message }),
    sessionRewind: (id, message, files) =>
        invoke<RewindDone>("session_rewind", { id, message, files }),
    sessionFilesPutBack: (id) => invoke("session_files_put_back", { id }),
    taskFiles: (project, name, dir) => invoke("task_files", { project, name, dir }),
    taskOpenFile: (project, name, path) => invoke("task_open_file", { project, name, path }),
    taskOpenTerminal: (project, name) => invoke("task_open_terminal", { project, name }),
    taskOpenEditor: (project, name) => invoke("task_open_editor", { project, name }),
    taskOpenUrl: (project, name) => invoke("task_open_url", { project, name }),
    openLink: (url) => invoke("open_link", { url }),
    note: (project, task) => invoke<string>("note", { project, task }),
    setNote: (project, task, text) => invoke("set_note", { project, task, text }),
    apiKeys: () => invoke<ApiKeyInfo[]>("api_keys"),
    setApiKey: (name, key) => invoke("set_api_key", { name, key }),
    deleteApiKey: (name) => invoke("delete_api_key", { name }),
    settings: () => invoke<Settings>("settings"),
    configProblems: () => invoke<string[]>("config_problems"),
    setSettings: (settings) => invoke("set_settings", { settings }),
    branchExample: (prefix) => invoke<string>("branch_example", { prefix }),
    version: () => getVersion(),
    updateState: () => invoke<UpdateState>("update_state"),
    updateCheck: () => invoke<UpdateState>("update_check"),
    updateRestart: () => invoke("update_restart"),
    onUpdateState: (listener) =>
        listen<UpdateState>("update-state", (event) => listener(event.payload)),
    terminalConfig: () => invoke("terminal_config"),
    terminalConfigSave: (text) => invoke("terminal_config_save", { text }),
    customLayout: () => invoke("custom_layout"),
    customLayoutSave: (text) => invoke("custom_layout_save", { text }),
    stylesheet: () => invoke("stylesheet"),
    stylesheetSave: (text) => invoke("stylesheet_save", { text }),
    stylesheetAccept: (text) => invoke("stylesheet_accept", { text }),
    stylesheetOpen: () => invoke("stylesheet_open"),
    stylesheetLeftOut: (leftOut) => invoke("stylesheet_left_out", { leftOut }),
    setBadge: (count) => invoke("badge_set", { count }),
    onHookOutput: (listener) =>
        listen<HookOutput>("hook-output", (event) => listener(event.payload)),
    sessionFork: (id, from, agent) =>
        invoke<Session>("session_fork", { id, from, agent: agent ?? null }),
    sessionForkInto: (id, project, task) =>
        invoke<Session>("session_fork_into", { id, project, task }),
    sessionNew: (project, task, { agent, role, ...how }) =>
        invoke<Session>("session_new", { project, task, agent, role: role ?? null, how }),
    sessionEvents: (id) => invoke<AgentEvent[]>("session_events", { id }),
    sessionSend: (id, text, files, sessions, github, elements) =>
        invoke("session_send", { id, text, ...attached(files, sessions, github, elements) }),
    sessionReachable: (id) => invoke<Reachable[]>("session_reachable", { id }),
    projectReachable: (project) => invoke<Reachable[]>("project_reachable", { project }),
    projectGithubItems: (project, query) =>
        invoke<GitHubItems>("project_github_items", { project, query }),
    projectGithubRead: (project, numbers) =>
        invoke<GitHubItem[]>("project_github_read", { project, numbers }),
    sessionImagesKeep: (id, images) => invoke<string[]>("session_images_keep", { id, images }),
    sessionImage: (id, path) => invoke<string>("session_image", { id, path }),
    sessionImageOpen: (id, path) => invoke<void>("session_image_open", { id, path }),
    attachmentsRoot: () => invoke<string>("attachments_root"),
    sessionTellNow: (id, message) => invoke<void>("session_tell_now", { id, message }),
    sessionMessageNow: (id, text, files, sessions, github, elements) =>
        invoke("session_message_now", {
            id,
            text,
            ...attached(files, sessions, github, elements),
        }),
    sessionQueue: (id) => invoke<QueuedMessage[]>("session_queue", { id }),
    sessionEnqueue: (id, text, files, sessions, github, elements) =>
        invoke<QueuedMessage[]>("session_enqueue", {
            id,
            text,
            ...attached(files, sessions, github, elements),
        }),
    sessionQueueEdit: (id, message, text, files, sessions, github, elements) =>
        invoke<QueuedMessage[]>("session_queue_edit", {
            id,
            message,
            text,
            ...attached(files, sessions, github, elements),
        }),
    sessionQueueHold: (id, message) => invoke<boolean>("session_queue_hold", { id, message }),
    sessionQueueLetGo: (id, message) => invoke<void>("session_queue_let_go", { id, message }),
    sessionSendQueued: (id) => invoke("session_send_queued", { id }),
    sessionSendNow: (id, message) => invoke("session_send_now", { id, message }),
    sessionMarkRead: (id) => invoke("session_mark_read", { id }),
    sessionAllow: (id, requestId) => invoke("session_allow", { id, requestId }),
    sessionAnswer: (id, requestId, answers) => invoke("session_answer", { id, requestId, answers }),
    configAnswer: (requestId, accepted) => invoke("config_answer", { requestId, accepted }),
    sessionDeny: (id, requestId, reason) => invoke("session_deny", { id, requestId, reason }),
    sessionInterrupt: (id) => invoke("session_interrupt", { id }),
    taskBackground: (project, name) => invoke("task_background", { project, name }),
    sessionStopBackground: (id, taskId) => invoke("session_stop_background", { id, taskId }),
    sessionSendToBackground: (id, toolUseId) =>
        invoke<boolean>("session_send_to_background", { id, toolUseId }),
    sessionBackgroundOutput: (id, taskId, from) =>
        invoke<BackgroundOutput | null>("session_background_output", { id, taskId, from }),
    slashCommands: (id) => invoke<SlashCommand[]>("slash_commands", { id }),
    sessionCommandAnswer: (id, requestId, answers) =>
        invoke("session_command_answer", { id, requestId, answers }),
    sessionSetAgent: (id, agent) => invoke<Session>("session_set_agent", { id, agent }),
    sessionConfigure: (id, settings) => invoke<Session>("session_configure", { id, how: settings }),
    viewUrl: async (document) => {
        const id = await invoke<string>("view_put", { document });
        return `swarmed-view://localhost/${id}`;
    },
    viewOpen: (document) => invoke("view_open", { document }),
    appUrl: async (id, toolUseId, document, csp) => {
        const shown = await invoke<AppShown>("app_put", { id, toolUseId, document, csp });
        return { ...shown, address: `swarmed-view://app-${shown.id}/` };
    },
    sessionAppCall: (id, toolUseId, tool, args) =>
        invoke<unknown>("session_app_call", { id, toolUseId, tool, arguments: args }),
    terminalOpen: (id, cols, rows) => invoke<TerminalOpened>("terminal_open", { id, cols, rows }),
    terminalWrite: (id, data) => invoke("terminal_write", { id, data }),
    terminalResize: (id, cols, rows) => invoke("terminal_resize", { id, cols, rows }),
    onTerminalOutput: (listener) =>
        listen<TerminalOutput>("terminal-output", (event) => listener(event.payload)),
    browserSetup: () => invoke<BrowserSetup>("browser_setup"),
    browserInstall: () => invoke<BrowserSetup>("browser_install"),
    browserState: (taskId) => invoke<BrowserState>("browser_state", { taskId }),
    browserOpen: (taskId) => invoke<BrowserState>("browser_open", { taskId }),
    browserStop: (taskId) => invoke("browser_stop", { taskId }),
    browserShow: (taskId, tab, viewport) =>
        invoke<BrowserState>("browser_show", { taskId, tab, viewport }),
    browserHide: (taskId) => invoke("browser_hide", { taskId }),
    browserRaise: (taskId, tab) => invoke("browser_raise", { taskId, tab }),
    browserMouse: (taskId, input) => invoke("browser_mouse", { taskId, input }),
    browserKey: (taskId, input) => invoke("browser_key", { taskId, input }),
    browserText: (taskId, text) => invoke("browser_text", { taskId, text }),
    browserGo: (taskId, tab, go) => invoke("browser_go", { taskId, tab, go }),
    browserTabNew: (taskId, url) => invoke<string>("browser_tab_new", { taskId, url }),
    browserTabClose: (taskId, tab) => invoke("browser_tab_close", { taskId, tab }),
    browserDialog: (taskId, tab, accept, text) =>
        invoke("browser_dialog", { taskId, tab, accept, text }),
    browserConsole: (taskId, tab) => invoke<ConsoleLine[]>("browser_console", { taskId, tab }),
    browserPick: (taskId, sessionId) =>
        invoke<PickedElement | null>("browser_pick", { taskId, sessionId }),
    browserPickEnd: (taskId) => invoke("browser_pick_end", { taskId }),
    sessionElements: (id, numbers) => invoke<PickedElement[]>("session_elements", { id, numbers }),
    onBrowserEvent: (listener) =>
        listen<BrowserEvent>("browser-event", (event) => listener(event.payload)),
    claudeTerminalOpen: (id, command, cols, rows) =>
        invoke<TerminalOpened>("claude_terminal_open", { id, command, cols, rows }),
    claudeTerminalWrite: (id, data) => invoke("claude_terminal_write", { id, data }),
    claudeTerminalResize: (id, cols, rows) => invoke("claude_terminal_resize", { id, cols, rows }),
    claudeTerminalClose: (id) => invoke("claude_terminal_close", { id }),
    onClaudeTerminalOutput: (listener) =>
        listen<TerminalOutput>("claude-terminal-output", (event) => listener(event.payload)),
    statusLine: (id) => invoke<string | null>("status_line", { id }),
    accountUsage: () => invoke<AccountUsage>("account_usage"),
    sessionUsageRefresh: (id) => invoke<UsageAsk>("session_usage_refresh", { id }),
    sessionRemoteControl: (id, on) => invoke("session_remote_control", { id, on }),
    sessionContinueAtReset: (id) => invoke<number>("session_continue_at_reset", { id }),
    sessionContinueCancel: (id) => invoke("session_continue_cancel", { id }),
    sessionRename: (id, title) => invoke<Session>("session_rename", { id, title }),
    sessionSetParent: (id, parent) => invoke<Session>("session_set_parent", { id, parent }),
    roles: () => invoke<RoleInfo[]>("roles"),
    sessionSetRole: (id, role) => invoke<Session>("session_set_role", { id, role }),
    sessionMark: (id, colour, label) => invoke<Session>("session_mark", { id, colour, label }),
    sessionClose: (id, withWorkers) => invoke<number[]>("session_close", { id, withWorkers }),
    sessionDescendants: (id, closed) => invoke<Descendant[]>("session_descendants", { id, closed }),
    sessionDelete: (id) => invoke("session_delete", { id }),
    sessionsSearch: (query) => invoke<SearchHit[]>("sessions_search", { query }),
    sessionsClosed: (project, task) => invoke<Session[]>("sessions_closed", { project, task }),
    sessionReopen: (id) => invoke<Session>("session_reopen", { id }),
    onSessionEvent: (listener) =>
        listen<SessionEvent>("session-event", (event) => listener(event.payload)),
    onNoticeOpened: (listener) =>
        listen<number>("notice-opened", (event) => listener(event.payload)),
    onEventsLagged: (listener) => listen("session-events-lagged", () => listener()),
    onMenu: (listener) => listen<string>("menu", (event) => listener(event.payload)),
};

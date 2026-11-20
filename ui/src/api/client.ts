import type {
    Action,
    AgentEvent,
    AgentInfo,
    AgentSetup,
    ApiKeyInfo,
    BackgroundTask,
    BranchChoices,
    CloudProvider,
    CloudTask,
    Costs,
    Environment,
    FileChange,
    FileDiff,
    HookOutput,
    MarkColour,
    McpServerInput,
    McpServerView,
    Project,
    Pull,
    PullDraft,
    QueuedMessage,
    SearchHit,
    Session,
    SessionEvent,
    SessionSettings,
    SessionSetup,
    Settings,
    Skill,
    Snapshot,
    Task,
    TaskNaming,
    Template,
    TerminalOutput,
    TurnStats,
    Unpublished,
    Usage,
} from "./types";

/** Stops a subscription. */
export type Unsubscribe = () => void;

/** Everything the UI asks of the app. The Tauri shell implements it; so does the mock. */
export interface Api {
    overview(): Promise<Project[]>;
    /** The agents a session can run on, Claude Code first. */
    agents(): Promise<AgentInfo[]>;
    /** Every MCP server Swarmed knows of: those set up here, then Claude Code's own. */
    mcpServers(): Promise<McpServerView[]>;
    /** Adds a server set up here, or replaces the one of that name. Returns the new list. */
    mcpSave(server: McpServerInput): Promise<McpServerView[]>;
    /** Removes a server set up here, with its secrets. Returns the new list. */
    mcpRemove(name: string): Promise<McpServerView[]>;
    /** What Swarmed takes up and uses. Measuring the worktrees can take a few seconds. */
    /** The actions offered in `project`: those for every repository, then its own. */
    actions(project: string | null): Promise<Action[]>;
    /** Keeps an action, in `project`'s own or for every repository; returns what is offered. */
    actionSave(project: string | null, action: Action): Promise<Action[]>;
    /** Drops an action; returns what is then offered. */
    actionRemove(project: string | null, name: string): Promise<Action[]>;
    /** The prompts the user keeps to send again, by name. */
    templates(): Promise<Template[]>;
    /** Keeps a prompt, in place of the one of that name; returns them all. */
    templateSave(name: string, text: string): Promise<Template[]>;
    /** Drops a kept prompt; returns the rest. */
    templateRemove(name: string): Promise<Template[]>;
    usage(): Promise<Usage>;
    /** What the agents report their work has cost: by day and by task. */
    costs(): Promise<Costs>;
    /** The project's snapshots: what new tasks of each environment start from. */
    snapshots(project: string): Promise<Snapshot[]>;
    /** Keeps what the task's setup produced as the snapshot of its environment. */
    snapshotSave(project: string, task: string): Promise<Snapshot>;
    /** Forgets the snapshot of an environment; `null` is the one of tasks without any. */
    snapshotRemove(project: string, environment: string | null): Promise<void>;
    /** The sessions' turns added up by day, task, agent and model, the oldest day first. */
    analytics(): Promise<TurnStats[]>;
    /**
     * The effort an agent last ran a model with when the effort was left to it; `null` for
     * `model` is the agent's default model. `null` if no such session has run yet.
     */
    effortDefault(agent: string, model: string | null): Promise<string | null>;
    /** The agents that can be set up from the settings, and how far along each is. */
    agentSetups(): Promise<AgentSetup[]>;
    /** Installs a known agent and configures it. Its installer's output comes as hook output. */
    agentInstall(id: string): Promise<AgentSetup>;
    /** Makes an installed agent usable, offering `models` if given, else keeping its own. */
    agentConfigure(id: string, models: string[] | null): Promise<AgentSetup>;
    /** Asks an agent that knows its own models which ones it offers, and offers those. */
    agentModelsRefresh(id: string): Promise<AgentSetup>;
    /** Every model OpenRouter offers right now, named as agents take them. */
    openrouterModels(): Promise<string[]>;
    /** Lets the user choose files to point an agent at; empty if they cancelled. */
    pickFiles(): Promise<string[]>;
    /** Calls `listener` with the paths of files dropped on the window, from now on. */
    onFilesDropped(listener: (files: string[]) => void): Promise<Unsubscribe>;
    /** Lets the user choose a folder; `null` if they cancelled. */
    pickFolder(): Promise<string | null>;
    /** Registers the git repository containing `path`. */
    projectAdd(path: string): Promise<Project>;
    /**
     * Takes a repository out of Swarmed: its tasks' worktrees, its sessions and the notes on it
     * are deleted, the repository itself is not touched. Without `force` it refuses if a task
     * holds unsaved work.
     */
    projectRemove(name: string, force: boolean): Promise<void>;
    /** Removes a task's worktree and takes the task out of the tree. Its branch is kept. */
    taskRemove(project: string, name: string, force: boolean): Promise<void>;
    /** What a new task in the project can start from. */
    branchChoices(project: string): Promise<BranchChoices>;
    /**
     * A name for a task that starts with `prompt`, and a title for its first session, from the
     * model that names sessions. `null` if naming is off or no answer came in time: the task is
     * then named after the prompt's first words.
     */
    taskNameFor(project: string, prompt: string): Promise<TaskNaming | null>;
    /**
     * Creates a task: a worktree on a new branch, or on `branch` if one is given, then the
     * project's `setup` hook. This can take a while.
     */
    taskNew(
        project: string,
        name: string,
        branch: string | null,
        environment: string | null,
    ): Promise<Task>;
    /**
     * Hands a prompt to a cloud agent working on the project's repository, from the branch of
     * `task` if one is named and it has been pushed. Rejects, saying what to do, if the
     * repository is not on GitHub or has no cloud environment.
     */
    cloudDelegate(
        project: string,
        prompt: string,
        task: string | null,
        provider: CloudProvider,
    ): Promise<CloudTask>;
    /** The project's cloud tasks; with `refresh`, their statuses are asked for again first. */
    cloudTasks(project: string, refresh: boolean): Promise<CloudTask[]>;
    /** Takes a cloud task off the list, without stopping it. */
    cloudTaskForget(id: number): Promise<void>;
    /** Opens a cloud task's page at its provider. */
    cloudTaskOpen(id: number): Promise<void>;
    /** What a Codex cloud task changed, as a unified diff. Refused while it has none. */
    cloudTaskDiff(id: number): Promise<string>;
    /** Applies what a Codex cloud task changed to a local task's worktree, uncommitted. */
    cloudTaskApply(id: number, project: string, name: string): Promise<void>;
    /** Starts what a task keeps running. Says whether its project has a start command at all. */
    stackStart(project: string, task: string): Promise<boolean>;
    /** Stops what a task keeps running. */
    stackStop(project: string, task: string): Promise<void>;
    /** The end of what a task's stack has printed since it was last started. */
    stackLog(project: string, task: string): Promise<string>;
    /** Every skill Swarmed knows of: its own first, then each agent's. */
    skills(): Promise<Skill[]>;
    /** What a skill of Swarmed's own says to do, for editing. */
    skillBody(name: string): Promise<string>;
    /** Adds a skill of Swarmed's own, or replaces the one of that name. Gives all skills. */
    skillSave(name: string, description: string, body: string): Promise<Skill[]>;
    skillRemove(name: string): Promise<Skill[]>;
    /** Says whether a skill is offered to sessions whose agent lacks it. Gives all skills. */
    skillShare(origin: string, name: string, shared: boolean): Promise<Skill[]>;
    /** A project's environments, by name. */
    environments(project: string): Promise<Record<string, Environment>>;
    /** Adds an environment to a project, or replaces the one of that name. */
    environmentSave(project: string, name: string, environment: Environment): Promise<void>;
    environmentRemove(project: string, name: string): Promise<void>;
    /** The files the task has changed since its branch left its base, uncommitted ones included. */
    /** What of a task's work is not committed, and what is not pushed. */
    taskUnpublished(project: string, name: string): Promise<Unpublished>;
    /**
     * A commit message written for a task's uncommitted work, by the agent of `session` or,
     * without one or for Claude Code, by Claude Code's smallest model. Commits nothing.
     */
    taskCommitMessage(project: string, name: string, session: number | null): Promise<string>;
    /** A title and description written for the task's pull request. Opens nothing. */
    taskPullMessage(project: string, name: string, session: number | null): Promise<PullDraft>;
    /** Commits everything in a task's worktree under `message`. */
    taskCommit(project: string, name: string, message: string): Promise<void>;
    /** Pushes a task's branch to its remote. */
    taskPush(project: string, name: string): Promise<void>;
    /** A title and description to start a task's pull request from: its commits. */
    taskPullDraft(project: string, name: string): Promise<PullDraft>;
    /** Opens a pull request for a task's branch, as a draft if asked. */
    taskPullCreate(
        project: string,
        name: string,
        draft: PullDraft,
        asDraft: boolean,
    ): Promise<Pull>;
    /** The pull request of a task's branch with its checks; `null` if it has none. */
    taskPull(project: string, name: string): Promise<Pull | null>;
    /** Opens a task's pull request in the browser, or the details of its check `check`. */
    taskPullOpen(project: string, name: string, check: string | null): Promise<void>;
    taskChanges(project: string, name: string): Promise<FileChange[]>;
    /** One changed file's diff. */
    taskDiff(project: string, name: string, path: string): Promise<FileDiff>;
    /** Opens the task's worktree, with its branch checked out, in the user's editor. */
    taskOpenEditor(project: string, name: string): Promise<void>;
    /** Opens the task's running stack in the default browser. */
    taskOpenUrl(project: string, name: string): Promise<void>;
    /** Opens a web or mail link in the user's browser, never in Swarmed's own window. */
    openLink(url: string): Promise<void>;
    /** The user's note on a repository, or on one of its tasks; empty if there is none. */
    note(project: string, task: string | null): Promise<string>;
    /** Replaces that note. Nothing but blank space removes it. */
    setNote(project: string, task: string | null, text: string): Promise<void>;
    /** The providers a key can be stored for, and which have one. */
    apiKeys(): Promise<ApiKeyInfo[]>;
    /** Stores a provider's key in the system's keychain. It cannot be read back. */
    setApiKey(name: string, key: string): Promise<void>;
    deleteApiKey(name: string): Promise<void>;
    settings(): Promise<Settings>;
    setSettings(settings: Settings): Promise<void>;
    /** Shows on the app's icon how many sessions wait for the user; nothing for none. */
    setBadge(count: number): Promise<void>;
    /** Calls `listener` for every line a hook prints while a task is created or removed. */
    onHookOutput(listener: (output: HookOutput) => void): Promise<Unsubscribe>;
    /** Creates a session in a task. Its agent starts with the first message. */
    sessionNew(project: string, task: string, setup: SessionSetup): Promise<Session>;
    /**
     * A new session in the same task that starts with what was said in session `id`: all of
     * it, or only its first `upto` events.
     */
    sessionFork(id: number, upto: number | null): Promise<Session>;
    /** Everything the session has said and been told, oldest first. */
    sessionEvents(id: number): Promise<AgentEvent[]>;
    /** Starts a turn, starting the session's agent first if it is not running. */
    sessionSend(id: number, text: string, files?: string[]): Promise<void>;
    /** The messages waiting for the session's agent to finish, first to be sent first. */
    sessionQueue(id: number): Promise<QueuedMessage[]>;
    /** Adds a message to the queue: it is sent by itself when the agent's turn ends well. */
    sessionEnqueue(id: number, text: string, files?: string[]): Promise<QueuedMessage[]>;
    /** Rewrites a waiting message; empty text takes it out of the queue. */
    sessionQueueEdit(id: number, message: number, text: string): Promise<QueuedMessage[]>;
    /** Sends the first waiting message now, for a queue a stopped turn left standing. */
    sessionSendQueued(id: number): Promise<void>;
    /** Says the user has looked at the session, so it is no longer unread. */
    sessionMarkRead(id: number): Promise<void>;
    sessionAllow(id: number, requestId: string): Promise<void>;
    /** Answers a change to Swarmed's configuration that an agent proposed. */
    configAnswer(requestId: string, accepted: boolean): Promise<void>;
    sessionDeny(id: number, requestId: string, reason: string): Promise<void>;
    /** Stops the current turn; the session stays usable. */
    sessionInterrupt(id: number): Promise<void>;
    /** What a task's agents left running in the background, the running ones first. */
    taskBackground(project: string, name: string): Promise<BackgroundTask[]>;
    /** Asks a session's agent to stop something it left running. */
    sessionStopBackground(id: number, taskId: string): Promise<void>;
    /**
     * The commands and skills a message can call by starting with `/`, as last reported by the
     * session's agent; for `null` or a session that has not run, by the newest session.
     */
    slashCommands(id: number | null): Promise<string[]>;
    /**
     * Changes the model and supervision. A running agent is told at once: the model applies
     * from its next turn, the supervision from its next action.
     */
    sessionConfigure(id: number, settings: SessionSettings): Promise<Session>;
    /**
     * Puts a session that has not begun on another agent, with that agent's default model.
     * Refused once the session has talked to its agent.
     */
    sessionSetAgent(id: number, agent: string): Promise<Session>;
    /**
     * Hands over an HTML document an agent wrote and returns the address a sandboxed frame
     * loads it from: an origin of its own, with no access to the app.
     */
    viewUrl(document: string): Promise<string>;
    /** Opens such a document in the user's browser, under the same restrictions as the frame. */
    viewOpen(document: string): Promise<void>;
    /**
     * Opens the session's terminal, starting a shell in the task's worktree if none runs, and
     * returns what it has printed so far, to redraw it.
     */
    terminalOpen(id: number, cols: number, rows: number): Promise<string>;
    /** Types into the session's terminal. */
    terminalWrite(id: number, data: string): Promise<void>;
    terminalResize(id: number, cols: number, rows: number): Promise<void>;
    /** Calls `listener` for everything any session's terminal prints, from now on. */
    onTerminalOutput(listener: (output: TerminalOutput) => void): Promise<Unsubscribe>;
    /**
     * What the user's status line script prints for the session, or `null` if no script is
     * configured and the built-in pieces apply.
     */
    statusLine(id: number): Promise<string | null>;
    /** Gives the session a title of the user's choosing. Every front end hears of it as an event. */
    sessionRename(id: number, title: string): Promise<Session>;
    /** Puts a mark on a session; the colour `none` takes it off. */
    sessionMark(id: number, colour: MarkColour | "none", label: string): Promise<Session>;
    /** A task's closed sessions, the one closed last first. */
    /** The sessions in which `query` was said, closed ones included. */
    sessionsSearch(query: string): Promise<SearchHit[]>;
    sessionsClosed(project: string, task: string): Promise<Session[]>;
    /** Puts a closed session back in the tree, with its conversation as it was. */
    sessionReopen(id: number): Promise<Session>;
    /** Stops the session's agent and takes the session out of the tree. Its history is kept. */
    sessionClose(id: number): Promise<void>;
    /** Calls `listener` for every event of every running session, from now on. */
    onSessionEvent(listener: (event: SessionEvent) => void): Promise<Unsubscribe>;
    /** Calls `listener` with the session a notification was about, when the user clicks it. */
    onNoticeOpened(listener: (sessionId: number) => void): Promise<Unsubscribe>;
    /** Calls `listener` when events were dropped and anything on screen should be reloaded. */
    onEventsLagged(listener: () => void): Promise<Unsubscribe>;
}

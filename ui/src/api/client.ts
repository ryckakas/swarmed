import type {
    AccountUsage,
    Action,
    AgentEvent,
    AgentInfo,
    AgentSetup,
    ApiKeyInfo,
    AppCsp,
    AppShown,
    BackgroundOutput,
    BackgroundTask,
    BranchChoices,
    BrowserEvent,
    BrowserGo,
    BrowserSetup,
    BrowserState,
    Cleaned,
    Cleanup,
    CleanupKind,
    CloudProvider,
    CloudTask,
    ConsoleLine,
    Costs,
    CustomLayout,
    Environment,
    EnvironmentChoice,
    FileChange,
    FileDiff,
    FileListing,
    GitHubItem,
    GitHubItems,
    HookOutput,
    Issue,
    IssueDetail,
    IssueFilter,
    IssueList,
    KeyInput,
    MarkColour,
    McpServerInput,
    McpServerView,
    MergeMethod,
    MergeReport,
    MergeRules,
    MergeWhen,
    MouseInput,
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
    SessionSettings,
    SessionSetup,
    Settings,
    SetupLog,
    Skill,
    SlashCommand,
    Snapshot,
    Task,
    TaskNaming,
    TaskStart,
    TaskStarted,
    Template,
    TerminalConfig,
    TerminalOpened,
    TerminalOutput,
    Turn,
    TurnChanges,
    TurnStats,
    Unpublished,
    UpdateState,
    Usage,
    UsageAsk,
    Viewport,
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
    /** Moves an action `by` places among those kept with it, earlier if negative; returns what is then offered. */
    actionMove(project: string | null, name: string, by: number): Promise<Action[]>;
    /** Drops an action; returns what is then offered. */
    actionRemove(project: string | null, name: string): Promise<Action[]>;
    /** The prompts the user keeps to send again, by name. */
    templates(): Promise<Template[]>;
    /** Keeps a prompt, in place of the one of that name; returns them all. */
    templateSave(name: string, text: string): Promise<Template[]>;
    /** Drops a kept prompt; returns the rest. */
    templateRemove(name: string): Promise<Template[]>;
    usage(): Promise<Usage>;
    /** What removed tasks left behind, and the snapshots, as can be cleaned up. */
    cleanup(): Promise<Cleanup>;
    /**
     * Deletes the things of one kind named by their keys, after listing them again: only
     * what is still listed goes. Returns what was deleted, and what was not with why.
     * `atRisk` names those of the keys that were shown as losing work: one that loses work
     * now and was not shown so is kept, since the list the user read may be old.
     */
    cleanupClean(kind: CleanupKind, keys: string[], atRisk: string[]): Promise<Cleaned>;
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
     * holds unsaved work. Resolves to what its tasks' removals passed over, as `taskRemove`.
     */
    projectRemove(name: string, force: boolean): Promise<string[]>;
    /** Moves a project `by` places among the others in the sidebar, earlier for a negative number. */
    projectMove(project: string, by: number): Promise<void>;
    /** Moves a task `by` places among its project's other open tasks. */
    taskMove(project: string, task: string, by: number): Promise<void>;
    /** Moves a session `by` places among its task's others, or among those its starter started. */
    sessionMove(id: number, by: number): Promise<void>;
    /** Moves a cloud task `by` places among the others of its project. */
    cloudTaskMove(id: number, by: number): Promise<void>;
    /**
     * Pins a project above the others that are not pinned, put last among the pinned ones, or
     * with `pinned` false unpins it, put first among the others.
     */
    projectPin(project: string, pinned: boolean): Promise<void>;
    /** Pins a task above its project's other open tasks that are not pinned, or unpins it. */
    taskPin(project: string, task: string, pinned: boolean): Promise<void>;
    /** Pins a session above its siblings that are not pinned, or unpins it. */
    sessionPin(id: number, pinned: boolean): Promise<void>;
    /** Highlights a task in `colour` where it is, or with `null` takes its highlight off. */
    taskHighlight(project: string, task: string, colour: MarkColour | null): Promise<void>;
    /** Highlights a session in `colour` where it is, or with `null` takes its highlight off. */
    sessionHighlight(id: number, colour: MarkColour | null): Promise<void>;
    /**
     * Removes a task's worktree and takes the task out of the tree. Its branch is kept. Without
     * `force` it refuses if the worktree holds unsaved work; nothing else stops it. Resolves to
     * what it passed over to get there, a teardown that failed say, for the user to read.
     */
    taskRemove(project: string, name: string, force: boolean): Promise<string[]>;
    /**
     * Puts a task away: stops its agents and its stack, closes its sessions if the settings say
     * so, and lists it with the done ones. Nothing is deleted. Resolves to what went wrong on
     * the way, a `stop` that failed say, for the user to read.
     */
    taskDone(project: string, name: string): Promise<string[]>;
    /** Brings a task that was put away back to the others. Nothing is started. */
    taskUndone(project: string, name: string): Promise<void>;
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
     * project's `setup` hook. Resolves once it is set up, which can take a while.
     */
    taskNew(
        project: string,
        name: string,
        branch: string | null,
        environment: EnvironmentChoice,
    ): Promise<Task>;
    /**
     * Starts a task from a prompt: a worktree on a new branch, or on `start.branch`, and its
     * first session, with the prompt waiting for the task's setup. Resolves once both are
     * written down, while the task is set up; how the setup goes arrives as `setup` events,
     * and the prompt goes once it finishes well. A name that is taken gives way to the first
     * free one, which the task returned has.
     */
    taskStart(project: string, name: string, start: TaskStart): Promise<TaskStarted>;
    /**
     * Runs a task's setup again; once it finishes well, what its sessions wait to send goes.
     * Resolves when it is over, and rejects with why it did not finish well.
     */
    taskSetup(project: string, name: string): Promise<Task>;
    /** Stops a task's setup, which leaves the task to be set up again. */
    taskSetupStop(project: string, name: string): Promise<void>;
    /**
     * Sends what a task's sessions wait to send until it is set up now, in the worktree as it
     * is. Resolves to the sessions it went to.
     */
    taskStartAnyway(project: string, name: string): Promise<number[]>;
    /** The end of what a task's last setup printed, and how it ended. */
    taskSetupLog(project: string, name: string): Promise<SetupLog>;
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
    /**
     * Hands the conversation of session `id` to a cloud agent, under what it is to do with it;
     * with no instruction it carries on where the conversation ended. Rejects like
     * `cloudDelegate`, and if nothing was said in the session.
     */
    cloudDelegateConversation(
        id: number,
        instruction: string,
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
    /** The environment a new task of the project is set up in when none is chosen, if any. */
    environmentDefault(project: string): Promise<string | null>;
    /** Makes an environment the default for new tasks; `null` leaves them to the project's own setup. */
    environmentSetDefault(project: string, name: string | null): Promise<void>;
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
    /** Pushes the branch a task's worktree is on to its remote. */
    taskPush(project: string, name: string): Promise<void>;
    /** Puts a task's worktree on another of the branches it has been on. */
    taskSwitch(project: string, name: string, branch: string): Promise<void>;
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
    /**
     * A message about a task's failed checks, with the last lines of their logs, for the user to
     * read and send to an agent. Refused when none failed.
     */
    taskPullFailure(project: string, name: string): Promise<string>;
    /** Opens a task's pull request in the browser, or the details of its check `check`. */
    taskPullOpen(project: string, name: string, check: string | null): Promise<void>;
    /**
     * The repository's GitHub issues that `filter` picks, the latest changed first, through the
     * user's own `gh`. Without `gh`, or off GitHub, the list is empty and says why.
     */
    projectIssues(project: string, filter: IssueFilter): Promise<IssueList>;
    /** One issue read whole, with its comments. Rejects with `gh`'s reason when it cannot be read. */
    projectIssue(project: string, issue: number): Promise<IssueDetail>;
    /** The issue a task was started for, read now; `null` for a task that is for none. */
    taskIssue(project: string, name: string): Promise<Issue | null>;
    /** What a repository allows for merging its pull requests. GitHub is asked once per run. */
    projectMergeRules(project: string): Promise<MergeRules>;
    /**
     * Merges a task's pull request with `method`, `when` asked, if its head is still `head`, the
     * one the user saw. No branch is deleted.
     */
    taskPullMerge(
        project: string,
        name: string,
        method: MergeMethod,
        when: MergeWhen,
        head: string,
    ): Promise<MergeReport>;
    /** Turns off the auto-merge of a task's pull request. */
    taskPullAutoMergeOff(project: string, name: string): Promise<MergeReport>;
    taskChanges(project: string, name: string): Promise<FileChange[]>;
    /** One changed file's diff. */
    taskDiff(project: string, name: string, path: string): Promise<FileDiff>;
    /** The turns of a session whose checkpoints are kept, oldest first. */
    sessionTurns(id: number): Promise<Turn[]>;
    /** What one turn of a session changed, by the number of the message that began it. */
    sessionTurnChanges(id: number, message: number): Promise<TurnChanges>;
    /** How one file changed in one turn of a session. */
    sessionTurnDiff(id: number, message: number, path: string): Promise<FileDiff>;
    /**
     * What taking session `id` back to before its message number `message` would do now:
     * whether it can, with its files or without, and what would change. Changes nothing.
     */
    sessionRewindCheck(id: number, message: number): Promise<RewindCheck>;
    /**
     * Takes session `id` back to before its message number `message`: that message and what
     * followed no longer count, and its agent goes on from there. With `files`, the worktree's
     * files go back to how they were before it too.
     */
    sessionRewind(id: number, message: number, files: boolean): Promise<RewindDone>;
    /** Puts back the files session `id`'s latest rewind took back. */
    sessionFilesPutBack(id: number): Promise<void>;
    /** Opens the task's worktree, with its branch checked out, in the user's editor. */
    taskOpenEditor(project: string, name: string): Promise<void>;
    /** What the folder `dir` of a task's worktree holds; the worktree itself for an empty `dir`. */
    taskFiles(project: string, name: string, dir: string): Promise<FileListing>;
    /** Opens a file of a task's worktree in the user's editor. */
    taskOpenFile(project: string, name: string, path: string): Promise<void>;
    /** Opens the task's worktree in the terminal application the user chose. */
    taskOpenTerminal(project: string, name: string): Promise<void>;
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
    /**
     * What of Swarmed's configuration could not be understood, each said for the user and led
     * by its file. What was understood is in use.
     */
    configProblems(): Promise<string[]>;
    /**
     * Replaces the settings. A branch prefix other than the one kept is checked by git's rules
     * first, and refused with git's reason.
     */
    setSettings(settings: Settings): Promise<void>;
    /**
     * The branch a task called fix-login would get with `prefix` as the branch prefix: the
     * login's when it is empty. Rejected with git's reason when git would not take it.
     */
    branchExample(prefix: string): Promise<string>;
    /** The installed app's version, such as `0.1.0`. */
    version(): Promise<string>;
    /** Where updating Swarmed stands. */
    updateState(): Promise<UpdateState>;
    /** Looks for a new version now; with automatic updates on, downloads it too. */
    updateCheck(): Promise<UpdateState>;
    /** Downloads the new version if need be, and restarts Swarmed to install it. */
    updateRestart(): Promise<void>;
    /** Calls `listener` whenever where updating stands changes. */
    onUpdateState(listener: (state: UpdateState) => void): Promise<Unsubscribe>;
    /** The user's configuration of the terminal. */
    terminalConfig(): Promise<TerminalConfig>;
    /** Keeps `text`, JSON as written, as the terminal's configuration; fails with the reason if it cannot be used. */
    terminalConfigSave(text: string): Promise<TerminalConfig>;
    /** The user's custom layout. Fails if its file was edited into something unusable. */
    customLayout(): Promise<CustomLayout>;
    /** Keeps `text`, JSON as written, as the custom layout; fails with the reason if it cannot be drawn. */
    customLayoutSave(text: string): Promise<CustomLayout>;
    /** Shows on the app's icon how many sessions wait for the user; nothing for none. */
    setBadge(count: number): Promise<void>;
    /** Calls `listener` for every line a hook prints while a task is created or removed. */
    onHookOutput(listener: (output: HookOutput) => void): Promise<Unsubscribe>;
    /** Creates a session in a task. Its agent starts with the first message. */
    sessionNew(project: string, task: string, setup: SessionSetup): Promise<Session>;
    /**
     * A new session in the same task that starts with what was said in session `id`: all of
     * it, or its message number `from` and what was said after it, counting from 0 every
     * message sent into it. With `agent`, the new session runs on that agent: a clone of the
     * conversation for another agent to carry on.
     */
    sessionFork(id: number, from: number | null, agent?: string): Promise<Session>;
    /**
     * A new session in task `task` of `project`, on the same agent and under the same title,
     * that starts with all that was said in session `id`: for a chat whose work belongs in a
     * repository. The first session stays as it is.
     */
    sessionForkInto(id: number, project: string, task: string): Promise<Session>;
    /** Everything the session has said and been told, oldest first. */
    sessionEvents(id: number): Promise<AgentEvent[]>;
    /**
     * Starts a turn, starting the session's agent first if it is not running. `files` are
     * listed under the message by their paths, and the `sessions` and the pull requests and
     * issues (`github`, by number) it mentions under the words before them; a session the
     * session cannot read, or a number nobody read (`projectGithubRead`), is refused before
     * anything is sent.
     */
    sessionSend(
        id: number,
        text: string,
        files?: string[],
        sessions?: number[],
        github?: number[],
        elements?: number[],
    ): Promise<void>;
    /**
     * The sessions session `id` can mention with `@`: those its agent can read, those of its
     * own task first, then the latest at work.
     */
    sessionReachable(id: number): Promise<Reachable[]>;
    /** The sessions a new task's first message in `project` can mention, the latest at work first. */
    projectReachable(project: string): Promise<Reachable[]>;
    /**
     * What `#` offers in a repository for `query`, through the user's `gh`: with nothing typed
     * its latest changed pull requests and issues, for a number that one, for words those
     * GitHub's search finds; or why there are none. Kept by the core, so that a message
     * mentioning one goes without asking again.
     */
    projectGithubItems(project: string, query: string): Promise<GitHubItems>;
    /**
     * Reads the pull requests and issues `numbers` of a repository unless they were read a
     * moment ago: asked before a message mentioning them is sent, which then waits on nothing.
     */
    projectGithubRead(project: string, numbers: number[]): Promise<GitHubItem[]>;
    /**
     * Keeps images pasted into a message to the session, each in base64, as files of the
     * session's own, and resolves to their paths, to attach to the message like any file.
     * Rejects, saying why, an image in a format agents do not read or too large.
     */
    sessionImagesKeep(id: number, images: string[]): Promise<string[]>;
    /** An image pasted into the session, as a `data:` address to show it from. */
    sessionImage(id: number, path: string): Promise<string>;
    /** Opens an image pasted into the session in the system's viewer. */
    sessionImageOpen(id: number, path: string): Promise<void>;
    /** The folder that holds every session's pasted images, a folder named by its id each. */
    attachmentsRoot(): Promise<string>;
    /**
     * Tells a waiting message to the agent now, in the middle of its turn, without stopping
     * it: only for an agent that `hears_mid_turn`.
     */
    sessionTellNow(id: number, message: number): Promise<void>;
    /** Sends a message now: a turn under way is stopped, and it goes ahead of anything waiting. */
    sessionMessageNow(
        id: number,
        text: string,
        files?: string[],
        sessions?: number[],
        github?: number[],
        elements?: number[],
    ): Promise<void>;
    /** The messages waiting for the session's agent to finish, first to be sent first. */
    sessionQueue(id: number): Promise<QueuedMessage[]>;
    /**
     * Adds a message to the queue: it is sent by itself when the agent's turn ends well or is
     * stopped.
     */
    sessionEnqueue(
        id: number,
        text: string,
        files?: string[],
        sessions?: number[],
        github?: number[],
        elements?: number[],
    ): Promise<QueuedMessage[]>;
    /**
     * Rewrites a waiting message, with its files and the sessions it mentions listed under it
     * as when it was queued; no text and nothing attached take it out of the queue.
     */
    sessionQueueEdit(
        id: number,
        message: number,
        text: string,
        files?: string[],
        sessions?: number[],
        github?: number[],
        elements?: number[],
    ): Promise<QueuedMessage[]>;
    /**
     * Holds a waiting message while it is edited, or renews the hold: while the first one is
     * held the queue sends nothing by itself. `false` once the message no longer waits.
     */
    sessionQueueHold(id: number, message: number): Promise<boolean>;
    /** Lets go of a waiting message left as it was; a send held back for it goes now. */
    sessionQueueLetGo(id: number, message: number): Promise<void>;
    /** Sends the first waiting message now, for a queue a failed turn or a stopped agent left. */
    sessionSendQueued(id: number): Promise<void>;
    /**
     * Sends a waiting message now, ahead of the others. A turn under way is stopped first and
     * the message goes as soon as it has ended.
     */
    sessionSendNow(id: number, message: number): Promise<void>;
    /** Says the user has looked at the session, so it is no longer unread. */
    sessionMarkRead(id: number): Promise<void>;
    sessionAllow(id: number, requestId: string): Promise<void>;
    /** Answers the questions an agent put to the user, each by its text. */
    sessionAnswer(id: number, requestId: string, answers: Record<string, string>): Promise<void>;
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
     * Asks a session's agent to send the command or sub-agent that call `toolUseId` waits on
     * to the background, or with `null` every one a call waits on: the call returns and the
     * turn goes on. `false` when it did not go: it had ended, or was there already.
     */
    sessionSendToBackground(id: number, toolUseId: string | null): Promise<boolean>;
    /**
     * What a command a session's agent left running printed: from `from` on, or its latest
     * part; `null` once the file is gone.
     */
    sessionBackgroundOutput(
        id: number,
        taskId: string,
        from: number | null,
    ): Promise<BackgroundOutput | null>;
    /**
     * The commands and skills a message can call by starting with `/`, as last reported by the
     * session's agent; for `null` or a session that has not run, by the newest session.
     */
    slashCommands(id: number | null): Promise<SlashCommand[]>;
    /**
     * Answers the questions a command such as `/model` put to the user, each by its text,
     * which makes the change. `null` puts them aside and changes nothing.
     */
    sessionCommandAnswer(
        id: number,
        requestId: string,
        answers: Record<string, string> | null,
    ): Promise<void>;
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
     * Makes the app an MCP server attached to a call ready for a frame, from the file it is
     * kept in, under the policy its server's declaration comes to: the address it loads from
     * (an origin of its own) and what it may reach.
     */
    appUrl(
        id: number,
        toolUseId: string,
        document: string,
        csp: AppCsp,
    ): Promise<AppShown & { address: string }>;
    /**
     * Calls a tool of its own server for the app shown under a call, through the session's
     * agent, which may ask the user first, and returns the tool's result.
     */
    sessionAppCall(id: number, toolUseId: string, tool: string, args: unknown): Promise<unknown>;
    /**
     * Opens the session's terminal, starting a shell in the task's worktree if none runs, and
     * returns what it has printed so far, to redraw it.
     */
    terminalOpen(id: number, cols: number, rows: number): Promise<TerminalOpened>;
    /** Types into the session's terminal. */
    terminalWrite(id: number, data: string): Promise<void>;
    terminalResize(id: number, cols: number, rows: number): Promise<void>;
    /** Calls `listener` for everything any session's terminal prints, from now on. */
    onTerminalOutput(listener: (output: TerminalOutput) => void): Promise<Unsubscribe>;
    /** The browser a task's Chrome runs on, or what would install one. */
    browserSetup(): Promise<BrowserSetup>;
    /** Installs Google Chrome with Homebrew, by the command `browserSetup` gives. */
    browserInstall(): Promise<BrowserSetup>;
    /** A task's browser as the pane shows it, without starting it. */
    browserState(taskId: number): Promise<BrowserState>;
    /** Starts the task's Chrome, or connects to the one that runs. */
    browserOpen(taskId: number): Promise<BrowserState>;
    /** Stops the task's Chrome. Its profile, with what was signed in to there, stays. */
    browserStop(taskId: number): Promise<void>;
    /**
     * Shows `tab` (the one shown, else the first, for `null`) at the room the pane gives it,
     * starting the task's Chrome if it does not run. Its pictures come from now on.
     */
    browserShow(taskId: number, tab: string | null, viewport: Viewport): Promise<BrowserState>;
    /** The pane is off screen: no more pictures. */
    browserHide(taskId: number): Promise<void>;
    /** Brings the window of a task's Chrome with a window of its own forward, on `tab`. */
    browserRaise(taskId: number, tab: string | null): Promise<void>;
    /** What the mouse did on the picture, passed on to the tab shown. */
    browserMouse(taskId: number, input: MouseInput): Promise<void>;
    /** A key on the picture, passed on to the tab shown. */
    browserKey(taskId: number, input: KeyInput): Promise<void>;
    /** Text pasted or composed on the picture, typed into the tab shown. */
    browserText(taskId: number, text: string): Promise<void>;
    /** Sends a tab to an address typed, back, forward, again, or stops it loading. */
    browserGo(taskId: number, tab: string, go: BrowserGo): Promise<void>;
    /** Opens a tab, on an address or empty, and returns its id. */
    browserTabNew(taskId: number, url: string | null): Promise<string>;
    browserTabClose(taskId: number, tab: string): Promise<void>;
    /** Answers the dialog a tab opened, with the text a `prompt` asks for. */
    browserDialog(taskId: number, tab: string, accept: boolean, text: string | null): Promise<void>;
    /** What a tab logged that is kept, oldest first. */
    browserConsole(taskId: number, tab: string): Promise<ConsoleLine[]>;
    /**
     * Puts the tab the pane shows into Chrome's inspect mode for a pick that goes to session
     * `sessionId`, and resolves once the user clicks an element: the element as read from
     * Chrome and kept with its picture in the session's folder. `null` when picking ended
     * without one: `browserPickEnd`, another pick, another tab shown, the browser gone.
     */
    browserPick(taskId: number, sessionId: number): Promise<PickedElement | null>;
    /** Ends picking in the task's browser without a pick. */
    browserPickEnd(taskId: number): Promise<void>;
    /** The elements picked for a session, by number, as they were kept. */
    sessionElements(id: number, numbers: number[]): Promise<PickedElement[]>;
    /** Calls `listener` for the pictures, tabs and console lines of every task's browser. */
    onBrowserEvent(listener: (event: BrowserEvent) => void): Promise<Unsubscribe>;
    /**
     * Opens Claude Code on the screen of `command`, named without its slash, in a terminal of
     * the session's own apart from its shell, or shows the one that runs on it. Nothing it
     * shows is given to the agent. Refused for a command Swarmed does its own way.
     */
    claudeTerminalOpen(
        id: number,
        command: string,
        cols: number,
        rows: number,
    ): Promise<TerminalOpened>;
    claudeTerminalWrite(id: number, data: string): Promise<void>;
    claudeTerminalResize(id: number, cols: number, rows: number): Promise<void>;
    /** Ends the Claude Code the session opened, if one runs. */
    claudeTerminalClose(id: number): Promise<void>;
    /** Calls `listener` for everything such a Claude Code prints, and when it ends. */
    onClaudeTerminalOutput(listener: (output: TerminalOutput) => void): Promise<Unsubscribe>;
    /**
     * What the user's status line script prints for the session, or `null` if no script is
     * configured and the built-in pieces apply.
     */
    statusLine(id: number): Promise<string | null>;
    /**
     * The account's usage as any Claude Code session last heard it. Asks Claude Code nothing;
     * changes arrive as `account_usage` events.
     */
    accountUsage(): Promise<AccountUsage>;
    /**
     * Asks the session's Claude Code for the account's usage now, starting it if it is not
     * running. Returns once asked, or once the question is set to go when Claude Code's own
     * copy of the figures is old enough for it to fetch them again, and says when; the plan's
     * limits and the extra usage arrive as the session's events, with nothing in the
     * conversation, and change the account's usage for every session. Only for a Claude Code
     * session.
     */
    sessionUsageRefresh(id: number): Promise<UsageAsk>;
    /**
     * Turns Remote Control of a Claude Code session on or off: whether it can be reached from
     * claude.ai and the Claude app. How it goes arrives as the session's `remote_control` events.
     */
    sessionRemoteControl(id: number, on: boolean): Promise<void>;
    /**
     * Lets a session a usage limit stopped go on by itself when the limit resets, and says when,
     * in seconds since 1970: a message is sent to it then. Every front end hears of it as a
     * `continuation` event. Refused when no limit still to reset stopped its latest turn.
     */
    sessionContinueAtReset(id: number): Promise<number>;
    /** Cancels a session's going on by itself when a usage limit resets. */
    sessionContinueCancel(id: number): Promise<void>;
    /** Gives the session a title of the user's choosing. Every front end hears of it as an event. */
    sessionRename(id: number, title: string): Promise<Session>;
    /**
     * Has a session work for another of the same repository, in any task, or with `null` for
     * nobody. Refused for a closed one on either side, and for one that works for it already,
     * however far up. Both agents are told with their next message.
     */
    sessionSetParent(id: number, parent: number | null): Promise<Session>;
    /** The roles a session can have, in the order they are offered. */
    roles(): Promise<RoleInfo[]>;
    /**
     * Gives a session the role called `role`, or with `null` takes its role off. Once something
     * was said in it, the change is recorded and its agent is told with the next message.
     */
    sessionSetRole(id: number, role: string | null): Promise<Session>;
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
    /** Deletes a session and its conversation for good, open or closed. */
    sessionDelete(id: number): Promise<void>;
    /** Calls `listener` for every event of every running session, from now on. */
    onSessionEvent(listener: (event: SessionEvent) => void): Promise<Unsubscribe>;
    /** Calls `listener` with the session a notification was about, when the user clicks it. */
    onNoticeOpened(listener: (sessionId: number) => void): Promise<Unsubscribe>;
    /** Calls `listener` when events were dropped and anything on screen should be reloaded. */
    onEventsLagged(listener: () => void): Promise<Unsubscribe>;
    /** Calls `listener` with the id of one of Swarmed's own items in the menu bar, when chosen. */
    onMenu(listener: (item: string) => void): Promise<Unsubscribe>;
}

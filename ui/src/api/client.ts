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
    SessionSettings,
    SessionSetup,
    Settings,
    Task,
    TerminalOutput,
} from "./types";

/** Stops a subscription. */
export type Unsubscribe = () => void;

/** Everything the UI asks of the app. The Tauri shell implements it; so does the mock. */
export interface Api {
    overview(): Promise<Project[]>;
    /** The agents a session can run on, Claude Code first. */
    agents(): Promise<AgentInfo[]>;
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
     * Creates a task: a worktree on a new branch, or on `branch` if one is given, then the
     * project's `setup` hook. This can take a while.
     */
    taskNew(project: string, name: string, branch: string | null): Promise<Task>;
    /** The files the task has changed since its branch left its base, uncommitted ones included. */
    taskChanges(project: string, name: string): Promise<FileChange[]>;
    /** One changed file's diff. */
    taskDiff(project: string, name: string, path: string): Promise<FileDiff>;
    /** Opens the task's worktree, with its branch checked out, in the user's editor. */
    taskOpenEditor(project: string, name: string): Promise<void>;
    /** Opens the task's running stack in the default browser. */
    taskOpenUrl(project: string, name: string): Promise<void>;
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
    /** Calls `listener` for every line a hook prints while a task is created or removed. */
    onHookOutput(listener: (output: HookOutput) => void): Promise<Unsubscribe>;
    /** Creates a session in a task. Its agent starts with the first message. */
    sessionNew(project: string, task: string, setup: SessionSetup): Promise<Session>;
    /** Everything the session has said and been told, oldest first. */
    sessionEvents(id: number): Promise<AgentEvent[]>;
    /** Starts a turn, starting the session's agent first if it is not running. */
    sessionSend(id: number, text: string): Promise<void>;
    sessionAllow(id: number, requestId: string): Promise<void>;
    sessionDeny(id: number, requestId: string, reason: string): Promise<void>;
    /** Stops the current turn; the session stays usable. */
    sessionInterrupt(id: number): Promise<void>;
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
    /** Stops the session's agent and takes the session out of the tree. Its history is kept. */
    sessionClose(id: number): Promise<void>;
    /** Calls `listener` for every event of every running session, from now on. */
    onSessionEvent(listener: (event: SessionEvent) => void): Promise<Unsubscribe>;
    /** Calls `listener` when events were dropped and anything on screen should be reloaded. */
    onEventsLagged(listener: () => void): Promise<Unsubscribe>;
}

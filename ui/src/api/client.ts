import type {
    AgentEvent,
    AgentInfo,
    BranchChoices,
    HookOutput,
    Project,
    Session,
    SessionEvent,
    SessionSettings,
    SessionSetup,
    Settings,
    Task,
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
    /** What a new task in the project can start from. */
    branchChoices(project: string): Promise<BranchChoices>;
    /**
     * Creates a task: a worktree on a new branch, or on `branch` if one is given, then the
     * project's `setup` hook. This can take a while.
     */
    taskNew(project: string, name: string, branch: string | null): Promise<Task>;
    /** Opens the task's worktree, with its branch checked out, in the user's editor. */
    taskOpenEditor(project: string, name: string): Promise<void>;
    /** Opens the task's running stack in the default browser. */
    taskOpenUrl(project: string, name: string): Promise<void>;
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
    /** Stops the session's agent and takes the session out of the tree. Its history is kept. */
    sessionClose(id: number): Promise<void>;
    /** Calls `listener` for every event of every running session, from now on. */
    onSessionEvent(listener: (event: SessionEvent) => void): Promise<Unsubscribe>;
    /** Calls `listener` when events were dropped and anything on screen should be reloaded. */
    onEventsLagged(listener: () => void): Promise<Unsubscribe>;
}

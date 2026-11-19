import type { AgentEvent, HookOutput, Project, Session, SessionEvent, Task } from "./types";

/** Stops a subscription. */
export type Unsubscribe = () => void;

/** Everything the UI asks of the app. The Tauri shell implements it; so does the mock. */
export interface Api {
    overview(): Promise<Project[]>;
    /** Lets the user choose a folder; `null` if they cancelled. */
    pickFolder(): Promise<string | null>;
    /** Registers the git repository containing `path`. */
    projectAdd(path: string): Promise<Project>;
    /** Creates a task: branch, worktree, then the project's `setup` hook. This can take a while. */
    taskNew(project: string, name: string): Promise<Task>;
    /** Calls `listener` for every line a hook prints while a task is created or removed. */
    onHookOutput(listener: (output: HookOutput) => void): Promise<Unsubscribe>;
    /** Creates a session in a task. Its agent starts with the first message. */
    sessionNew(project: string, task: string): Promise<Session>;
    /** Everything the session has said and been told, oldest first. */
    sessionEvents(id: number): Promise<AgentEvent[]>;
    /** Starts a turn, starting the session's agent first if it is not running. */
    sessionSend(id: number, text: string): Promise<void>;
    sessionAllow(id: number, requestId: string): Promise<void>;
    sessionDeny(id: number, requestId: string, reason: string): Promise<void>;
    /** Stops the current turn; the session stays usable. */
    sessionInterrupt(id: number): Promise<void>;
    /** Calls `listener` for every event of every running session, from now on. */
    onSessionEvent(listener: (event: SessionEvent) => void): Promise<Unsubscribe>;
    /** Calls `listener` when events were dropped and anything on screen should be reloaded. */
    onEventsLagged(listener: () => void): Promise<Unsubscribe>;
}

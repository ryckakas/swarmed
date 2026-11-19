// The shapes swarmed-core serialises. Field names are the core's, so they stay snake_case.

export type Supervision = "ask" | "auto" | "bypass";
export type SessionStatus = "idle" | "working" | "waiting" | "errored";
export type SetupStatus = "pending" | "ok" | "failed";

export interface Session {
    id: number;
    task_id: number;
    /** The id of the agent that runs the session. */
    agent: string;
    title: string;
    agent_session_id: string | null;
    model: string | null;
    supervision: Supervision;
    /** How hard the model thinks; `null` is the agent's default. */
    effort: Effort | null;
    status: SessionStatus;
}

export interface Task {
    id: number;
    project_id: number;
    name: string;
    branch: string;
    base_ref: string;
    worktree_path: string;
    slot: number;
    setup: SetupStatus;
    archived: boolean;
    sessions: Session[];
    /** Ids of the sessions whose agent is running. */
    running: number[];
    /** Where the task's running stack is reached in a browser, if its project says so. */
    url: string | null;
    /** Whether the user keeps a note on the task. */
    has_note: boolean;
}

/** One file a task has changed since its branch left its base. */
export interface FileChange {
    /** The path, relative to the worktree. */
    path: string;
    kind: "added" | "modified" | "deleted";
    /** Lines added and removed; `null` for a binary or very large file. */
    added: number | null;
    removed: number | null;
}

/** One file's changes as a unified diff. */
export interface FileDiff {
    text: string;
    /** Whether the diff was too long and its end is missing. */
    truncated: boolean;
}

/** One provider an API key can be stored for. The key itself never reaches the UI. */
export interface ApiKeyInfo {
    /** The environment variable agents read the key from. */
    name: string;
    provider: string;
    /** Whether a key is stored. */
    stored: boolean;
}

/** The colours the app is drawn in: the system's choice, or always one of the two. */
export type Theme = "system" | "dark" | "light";

/** One built-in piece of the status line. */
export type StatusItem = "branch" | "changes" | "model" | "context" | "cost" | "agent" | "worktree";

/** The user's own preferences. */
export interface Settings {
    /** The command that opens a folder in the user's code editor. */
    editor: string;
    /** Whether a model names a new session once its first message is sent. */
    auto_title: boolean;
    theme: Theme;
    /** The size of the app's text, in percent of its normal size. */
    text_scale: number;
    /** How much a new session's agent may do without asking, unless chosen otherwise. */
    default_supervision: Supervision;
    /** Whether agents may answer with small HTML views, and those are shown. */
    html_views: boolean;
    /** The built-in pieces of the status line under the message box, in order. */
    status_items: StatusItem[];
    /** A command whose first line of output replaces the built-in pieces. Empty for none. */
    status_command: string;
}

export interface Project {
    id: number;
    name: string;
    repo_path: string;
    tasks: Task[];
    /** Whether the user keeps a note on the repository. */
    has_note: boolean;
}

/** What a note is kept on: a repository, or one of its tasks. */
export interface NoteTarget {
    project: string;
    task: string | null;
}

export interface McpServerInfo {
    name: string;
    status: string;
}

/** One part of what fills an agent's context window. */
export interface ContextCategory {
    name: string;
    tokens: number;
    /** `used`, `free`, `buffer` or `deferred`, in the agent's words. */
    kind: string;
}

export type AgentEvent =
    | { type: "user_message"; text: string }
    | {
          type: "init";
          agent_session_id: string;
          model: string;
          permission_mode: string;
          api_key_source: string;
          mcp_servers: McpServerInfo[];
          skills: number;
          /** The commands and skills a message can call with `/`, without the slash. */
          slash_commands: string[];
      }
    | { type: "text"; text: string }
    /** A piece of a reply still being written; the whole reply follows as `text`. */
    | { type: "text_delta"; text: string }
    | { type: "tool_use"; id: string; name: string; input: unknown }
    | { type: "tool_result"; tool_use_id: string; content: string; is_error: boolean }
    | {
          type: "permission_request";
          request_id: string;
          tool_name: string;
          input: unknown;
          description: string | null;
      }
    /** How full the agent's context window is, reported after a turn. */
    | { type: "context_usage"; used: number; max: number; categories: ContextCategory[] }
    /** The session got a new title, from the user or from the model that names sessions. */
    | { type: "renamed"; title: string }
    /** What happened in the session's terminal, as given to the agent with the next message. */
    | { type: "terminal_shared"; text: string }
    | { type: "permission_answered"; request_id: string; allowed: boolean }
    | { type: "permission_withdrawn"; request_id: string }
    | { type: "rate_limit"; status: string; resets_at: number | null; raw: unknown }
    | { type: "turn_done"; is_error: boolean; result: string | null; cost_usd: number | null }
    | { type: "exited"; code: number | null; stderr: string }
    | { type: "other"; raw: unknown };

export interface SessionEvent {
    session_id: number;
    /** The session's status after this event, if the event changed it. */
    status: SessionStatus | null;
    event: AgentEvent;
}

/** A piece of what a session's terminal printed. */
export interface TerminalOutput {
    session_id: number;
    /** The text, with the escape sequences a terminal emulator expects. */
    data: string;
}

/** One line a hook printed, with the task it belongs to. */
export interface HookOutput {
    project: string;
    task: string;
    line: string;
}

/** What a new task in a project can start from. */
export interface BranchChoices {
    /** The ref a new branch would be created from. */
    base: string;
    /** Existing branches a task could check out instead. */
    existing: string[];
}

/** An agent sessions can run on. */
export interface AgentInfo {
    id: string;
    name: string;
    /** Model names worth offering for it. */
    models: string[];
}

/** How a new session should run. */
/** What can be changed about a session after it was created. */
export interface SessionSettings {
    /** A model name the agent accepts, or `null` for its default. */
    model: string | null;
    supervision: Supervision;
    /**
     * How hard the model thinks, or `null` for the agent's default. An agent takes it when it
     * starts, so a running one is restarted between turns and carries on the conversation.
     */
    effort: Effort | null;
}

/** How hard the model thinks before it answers. */
export type Effort = "low" | "medium" | "high" | "xhigh" | "max";

export interface SessionSetup {
    /** An agent id, or `null` for Claude Code. */
    agent: string | null;
    /** A model name the agent accepts, or `null` for its default. */
    model: string | null;
    supervision: Supervision;
    effort: Effort | null;
}

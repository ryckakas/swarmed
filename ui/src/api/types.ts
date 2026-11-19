// The shapes swarmed-core serialises. Field names are the core's, so they stay snake_case.

export type Supervision = "ask" | "auto" | "bypass" | "plan";
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
    /** Whether its agent finished a turn, or stopped with an error, that nobody has looked at. */
    unread: boolean;
    /** The session that started this one, if an agent did and not the user. */
    spawned_by: number | null;
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
    /** How many closed sessions the task has, which can be listed and reopened. */
    closed: number;
    /** The environment of its project the task was set up in, if any. */
    environment: string | null;
    /** Whether what the task keeps running is up; `null` if its project has no start command. */
    stack: boolean | null;
}

/**
 * One way of setting a project's tasks up on this machine. Whatever is left empty is as the
 * project's own configuration has it.
 */
export interface Environment {
    /** Runs once after a task's worktree is created, and has to finish. */
    setup: string;
    /** Started once setup is done and left running: a dev server and the like. */
    start: string;
    /** Runs when the stack is stopped, before what `start` left running is ended. */
    stop: string;
    /** Runs before a task's worktree is removed. */
    teardown: string;
    /** Where a task's running stack is reached in a browser. */
    open_url: string;
    /** Variables for the hooks and for the agents working in the task. */
    env: Record<string, string>;
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
/** A set of colours for the app's surfaces, text and links, each with a dark and a light form. */
export type Palette = "dusk" | "graphite" | "forest" | "plum" | "ember";

/** One built-in piece of the status line. */
export type StatusItem = "branch" | "changes" | "model" | "context" | "cost" | "agent" | "worktree";

/** The user's own preferences. */
export interface Settings {
    /** The command that opens a folder in the user's code editor. */
    editor: string;
    /** Whether a model names a new session once its first message is sent. */
    auto_title: boolean;
    theme: Theme;
    /** Which set of colours the app is drawn in, in dark and in light alike. */
    palette: Palette;
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
    /** The user's own rules, given to every agent whatever the model. Empty for none. */
    rules: string;
}

export interface Project {
    id: number;
    name: string;
    repo_path: string;
    tasks: Task[];
    /** Whether the user keeps a note on the repository. */
    has_note: boolean;
    /** The tasks handed to cloud agents from the repository, newest first, as last heard of. */
    cloud: CloudTask[];
}

/** Whose cloud a task can be handed to. */
export type CloudProvider = "codex" | "claude";

/** A task handed to an agent that runs in its provider's cloud. */
export interface CloudTask {
    id: number;
    project_id: number;
    /** Whose cloud it runs in. */
    provider: CloudProvider;
    /** The provider's id for it. */
    remote_id: string;
    /** Where it is looked at in a browser. */
    url: string;
    prompt: string;
    /** The branch it started from. */
    branch: string;
    /**
     * What the provider last said of it, such as `pending`, `ready` or `error`; `sent` for one
     * whose provider cannot be asked.
     */
    status: string;
    /** The session that delegated it, if an agent did. */
    session_id: number | null;
}

/** What is being removed: a repository, or one of its tasks. */
export type RemoveTarget = NoteTarget;
/** A message written while the agent was busy, waiting to be sent. */
export interface QueuedMessage {
    id: number;
    text: string;
}

/** What a new task and its first session are called, as a model named them. */
export interface TaskNaming {
    /** The task's name: free in its project, and fit for a branch and a folder. */
    name: string;
    /** The first session's title. */
    title: string;
}

/** An MCP server as the settings show it. Secrets are named, never shown. */
export interface McpServerView {
    name: string;
    /** `swarmed` for one set up here, `claude` for one of Claude Code's own. */
    origin: string;
    /** `stdio` for a command, `http` for an address, empty if Swarmed cannot tell. */
    kind: string;
    /** The command with its arguments, or the address. */
    target: string;
    secrets: string[];
    enabled: boolean;
    /** What Claude Code last said of it, such as `connected`; `null` if it has not. */
    status: string | null;
}

/** A server as it is saved. A secret whose value is `null` keeps the one stored. */
export interface McpServerInput {
    name: string;
    command: string;
    url: string;
    secrets: [name: string, value: string | null][];
    enabled: boolean;
}

/** What a group of processes takes right now. */
export interface ProcessUsage {
    processes: number;
    memory_bytes: number;
    /** In percent of one core. */
    cpu_percent: number;
}

/** What Swarmed takes up on disk, and what it is using right now. */
export interface Usage {
    /** The database with all sessions' events. */
    database_bytes: number;
    events: number;
    notes_bytes: number;
    /** Every active task's worktree, largest first; `bytes` is `null` if it was not measured. */
    worktrees: { project: string; task: string; path: string; bytes: number | null }[];
    /** Swarmed's own process with everything it started. */
    running: ProcessUsage;
    /** The share of that taken by each session whose agent is running. */
    sessions: ({ session_id: number; title: string } & ProcessUsage)[];
}

/** An agent that can be set up from the settings, and how far along it is. */
export interface AgentSetup {
    id: string;
    name: string;
    /** One sentence on what it is for. */
    about: string;
    /** Where its program was found; `null` if it is not installed. */
    installed: string | null;
    /** Whether sessions can run on it. */
    configured: boolean;
    /** The models offered for it. */
    models: string[];
    /** The command that installs it, shown before it is run. */
    install_command: string;
    /** How the user signs in to it, which Swarmed never does for them. */
    sign_in: string;
    /** Whether it is Claude Code: nothing to set up beyond installing, and no models to choose. */
    built_in: boolean;
    /** Where its models come from: built in, picked from OpenRouter's list, or told by the agent. */
    model_source: "built_in" | "openrouter" | "agent";
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
    /** The session was closed, by the user or by the session that started it. */
    | { type: "closed" }
    /** HTML the agent showed through Swarmed's own tool, handed over by the tool itself. */
    | { type: "view"; html: string }
    /** What the agent says it actually runs with, defaults resolved. */
    | { type: "applied"; model: string; effort: string | null }
    /** What the agent expects the user to say next, offered after a turn. */
    | { type: "suggestion"; text: string }
    /** What the session's queue of waiting messages now holds. Never part of the history. */
    | { type: "queue"; messages: QueuedMessage[] }
    /** The session's supervision changed: by choice, or because its plan was approved. */
    | { type: "supervision_changed"; supervision: Supervision }
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

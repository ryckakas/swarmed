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
    /** Whether the agent is asked for its fast mode, where it has one. */
    fast: boolean;
    /** The mark an agent or the user put on it, if any. */
    mark: Mark | null;
}

/** The colours a mark can have. Each theme draws its own. */
export type MarkColour = "green" | "blue" | "yellow" | "red" | "purple";

export const MARK_COLOURS: MarkColour[] = ["green", "blue", "yellow", "red", "purple"];

/** A colour and a few words on a session that say how its work stands. */
export interface Mark {
    colour: MarkColour;
    /** A few words beside the colour; may be empty. */
    label: string;
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
    /** Where the task's stack is opened by default: what its project says, or what it printed. */
    url: string | null;
    /** Every address the stack can be opened at, `url` first. */
    urls: string[];
    /** Whether the user keeps a note on the task. */
    has_note: boolean;
    /** How many closed sessions the task has, which can be listed and reopened. */
    closed: number;
    /** The environment of its project the task was set up in, if any. */
    environment: string | null;
    /** How far along what the task keeps running is; `null` if its project starts nothing. */
    stack: StackState | null;
}

/**
 * A stack whose command runs is "starting" until something answers at its address: it may
 * still be installing or building what it serves.
 */
export type StackState = "stopped" | "starting" | "running";

/** A button of the user's own: it sends a prompt to the agent or runs a command in the terminal. */
export interface Action {
    /** What the button says. */
    name: string;
    /** A sign shown before the name, such as an emoji; may be empty. */
    icon: string;
    /** Sent to the agent of the session on screen; empty for one that runs a command. */
    prompt: string;
    /** Typed into the session's terminal; empty for one that sends a prompt. */
    command: string;
    /** Whether it belongs to one repository and not to all. */
    of_project: boolean;
}

/** A prompt kept to be sent again. `{{something}}` in its text marks a place to fill in. */
export interface Template {
    name: string;
    text: string;
}

/** What the agents report their work has cost, in US dollars at API prices. */
export interface Costs {
    today: number;
    /** The last seven days, today included. */
    week: number;
    total: number;
    /** The most expensive tasks, the most expensive first. */
    tasks: { project: string; task: string; usd: number }[];
    /** The last days on which something was spent, the newest first. */
    days: { day: string; usd: number }[];
}

/** What a task's setup produced, kept for new tasks of its environment to start from. */
export interface Snapshot {
    /** The environment whose tasks start from it; `null` for tasks without one. */
    environment: string | null;
    /** The task it was taken from. */
    task: string;
    /** When it was taken, in seconds since 1970. */
    taken_at: number;
    /** How many files and folders git ignores were kept, counted at the top of each. */
    entries: number;
}

/** The turns of one day on one model of one agent in one task, added up. */
export interface TurnStats {
    /** The day, as `YYYY-MM-DD` in local time. */
    day: string;
    project: string;
    task: string;
    agent: string;
    model: string;
    turns: number;
    /** How many of the turns failed or were interrupted. */
    failed: number;
    /** In US dollars at API prices, as the agents report it. */
    usd: number;
    /** How long the turns took in all, as far as that was measured. */
    duration_ms: number;
}

/** One session in which the words searched for were said. */
export interface SearchHit {
    session_id: number;
    title: string;
    project: string;
    task: string;
    /** Whether the session was closed, and has to be reopened to be read. */
    closed: boolean;
    /** The newest place the words were said, with a little of what stands around them. */
    snippet: string;
    /** Whether the user said it, not the agent. */
    by_user: boolean;
    /** How many of the session's messages hold the words. */
    matches: number;
}

/** What of a task's work has not left its worktree yet. */
export interface Unpublished {
    /** How many files are changed, added or removed and not committed. */
    changed: number;
    /** How many commits are not on the branch's upstream, or its base if never pushed. */
    unpushed: number;
    /** Whether the branch has been pushed before. */
    pushed_before: boolean;
}

/** A title and a description to start a pull request from. */
export interface PullDraft {
    title: string;
    body: string;
}

/** One check on a pull request: a CI job, or a status another service reported. */
export interface Check {
    name: string;
    /** The workflow it is a job of, if it is one: "E2E" for its job "shard 3". */
    workflow: string | null;
    outcome: "passed" | "failed" | "running" | "skipped";
    /** Where its details are, if it said. */
    url: string | null;
}

/** The pull request of a task's branch, as the GitHub CLI reports it. */
export interface Pull {
    number: number;
    title: string;
    url: string;
    state: "open" | "merged" | "closed";
    draft: boolean;
    /** What its reviewers decided, such as `approved`; `null` if no review is asked for. */
    review: string | null;
    checks: Check[];
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
    /** Base ports by name: a task gets its own number for each as `{{port.<name>}}`. */
    ports: Record<string, number>;
    /** Variables for the hooks and for the agents working in the task. */
    env: Record<string, string>;
}

/** One file or folder in a folder of a task's worktree. */
export interface FileEntry {
    name: string;
    /** Whether it is a folder that can be listed in turn. */
    dir: boolean;
}

/** What one folder of a task's worktree holds: folders first, then files. */
export interface FileListing {
    entries: FileEntry[];
    /** How many more there are than are listed. */
    more: number;
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
export type Palette = "dusk" | "graphite" | "forest" | "plum" | "ember" | "swarm";
/**
 * The icons and words for sessions: plain ones, a swarm's, or a swarm's with its names for the
 * tree as well. Apart from the colours.
 */
export type Style = "plain" | "swarm" | "colony";

/** How large and how far apart things are drawn: dense, or made for easier reading. */
export type Layout = "classic" | "senior" | "custom";

/** Colours of one form of the app, by the stylesheet's names for them, each `#rrggbb`. */
export type LayoutColours = Record<string, string>;

/**
 * A layout of the user's own, as the core checked it: whatever it leaves out is drawn as in
 * the layout it starts from. Sizes are in pixels, the rest in percent of the classic layout's.
 */
export interface CustomLayout {
    base: "classic" | "senior";
    text?: { xs?: number; sm?: number; md?: number; lg?: number; xl?: number };
    line_height?: number;
    spacing?: number;
    icons?: number;
    row_height?: number;
    control_height?: number;
    font?: string;
    mono_font?: string;
    dark?: LayoutColours;
    light?: LayoutColours;
}

/**
 * The user's own configuration of the terminal, as the core checked it. Whatever it leaves out
 * is as Swarmed draws a terminal by itself.
 */
export interface TerminalConfig {
    font?: string;
    /** In pixels. */
    font_size?: number;
    /** In percent of the text's size. */
    line_height?: number;
    cursor?: "block" | "bar" | "underline";
    cursor_blink?: boolean;
    scrollback?: number;
    bold_is_bright?: boolean;
    option_is_meta?: boolean;
    copy_on_select?: boolean;
    /** Whether programs may put text on the clipboard. */
    clipboard?: boolean;
    gpu?: boolean;
    /** In tenths of a contrast ratio. */
    minimum_contrast?: number;
    dark?: Record<string, string>;
    light?: Record<string, string>;
}

/** One built-in piece of the status line. */
export type StatusItem =
    | "branch"
    | "changes"
    | "model"
    | "context"
    | "cost"
    | "agent"
    | "worktree"
    | "duration";

/** The user's own preferences. */
export interface Settings {
    /** The command that opens a folder in the user's code editor. */
    editor: string;
    /** The terminal application a task's worktree is opened in, by its name. */
    terminal_app: string;
    /** Whether a model names a new session once its first message is sent. */
    auto_title: boolean;
    theme: Theme;
    /** Which set of colours the app is drawn in, in dark and in light alike. */
    palette: Palette;
    /** Which icons and words the app uses, whatever its colours. */
    style: Style;
    /** How large and how far apart the app draws things, whatever its colours. */
    layout: Layout;
    /** The size of the app's text, in percent of what its layout starts from. */
    text_scale: number;
    /** How much a new session's agent may do without asking, unless chosen otherwise. */
    default_supervision: Supervision;
    /** Whether agents may answer with small HTML views, and those are shown. */
    html_views: boolean;
    /** Whether the system notifies when a session needs you, finishes or fails out of sight. */
    notifications: boolean;
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

/** A set of instructions for one kind of task, kept by an agent or by Swarmed itself. */
export interface Skill {
    name: string;
    /** When to use it, in its own words. */
    description: string;
    /** Whose it is: `swarmed`, an agent's id, or `agents` for the folder several agents share. */
    origin: string;
    /** The folder it lives in. */
    folder: string;
    /** Whether sessions whose agent lacks it are offered it. */
    shared: boolean;
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
    /** Removed tasks' worktrees that are not deleted yet, or could not be. */
    removed_bytes: number;
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

/** One of the plan's usage limits, as Claude Code reports it. */
export interface UsageLimit {
    /** `session` for the 5-hour window, `weekly_all`, or `weekly_scoped` for one model's. */
    kind: string;
    /** The model a limit of one model's is for. */
    model: string | null;
    /** How much of it is used, in percent. */
    percent: number;
    /** When it resets, as Claude Code gives it: ISO 8601. */
    resets_at: string | null;
    /** `normal`, or Claude Code's word for nearer. */
    severity: string;
}

/** One part of what fills an agent's context window. */
export interface ContextCategory {
    name: string;
    tokens: number;
    /** `used`, `free`, `buffer` or `deferred`, in the agent's words. */
    kind: string;
}

/** What Swarmed does with a command when it is sent. */
export type CommandKind = "prompt" | "screen" | "setting" | "terminal";

/** A command or skill a message can call by starting with `/`. */
export interface SlashCommand {
    /** The name, without its slash. */
    name: string;
    kind: CommandKind;
    /** For one that only works in a terminal, what to do in Swarmed instead; empty for none. */
    note: string;
}

/** One line of an agent's screen: a name, what it stands at, and what there is to add. */
export interface ScreenRow {
    label: string;
    value: string;
    /** How much is used, in percent, where the row is a measure. */
    share?: number | null;
    /** When what the row measures starts over, as the agent gave it (RFC 3339). */
    resets_at?: string | null;
    note?: string;
}

/** A part of a screen: a few rows under a heading, which may be empty. */
export interface ScreenSection {
    title: string;
    rows: ScreenRow[];
}

/** A question a command puts to the user, in the shape of the questions an agent asks. */
export interface AskedQuestion {
    question: string;
    header: string;
    options: { label: string; description: string }[];
    multiSelect: boolean;
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
    /** The agent asked for a change to Swarmed's configuration that the user has to agree to. */
    | { type: "config_proposed"; request_id: string; title: string; detail: string }
    /** The user answered a proposed change. */
    | { type: "config_answered"; request_id: string; accepted: boolean }
    /** Swarmed's configuration was changed from a session: what was read of it is out of date. */
    | { type: "config_changed"; kind: string }
    /** The session got a mark, or lost the one it had when `colour` is `null`. */
    | { type: "marked"; colour: MarkColour | null; label: string }
    /** The session was closed, by the user or by the session that started it. */
    | { type: "closed" }
    /** The session was deleted for good, with everything said in it. */
    | { type: "deleted" }
    /** What stands before this is a copy of what was said in the session forked from. */
    | { type: "forked"; from: number }
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
    | {
          /** The account's extra usage this billing period, in minor units of `currency`. */
          type: "extra_usage";
          enabled: boolean;
          used: number;
          limit: number | null;
          currency: string;
          decimals: number;
          limit_reached: boolean;
      }
    | {
          /** The plan's usage limits as Claude Code's `/usage` lists them, asked of it. */
          type: "usage_limits";
          limits: UsageLimit[];
      }
    | {
          type: "turn_done";
          is_error: boolean;
          result: string | null;
          cost_usd: number | null;
          /** How long the turn took, in milliseconds; absent from turns recorded before it was. */
          duration_ms?: number | null;
      }
    | { type: "exited"; code: number | null; stderr: string }
    | {
          type: "background";
          task_id: string;
          description: string;
          kind: string;
          state: BackgroundState;
          summary: string | null;
      }
    /** One of the agent's own screens, such as `/usage`, with the contents the agent gave. */
    | { type: "screen"; command: string; title: string; note: string; sections: ScreenSection[] }
    /** A command that changes a setting, such as `/model`, puts its choices to the user. */
    | { type: "command_asked"; request_id: string; command: string; questions: AskedQuestion[] }
    /** The user answered such a question or put it aside; `said` is what came of it. */
    | { type: "command_answered"; request_id: string; said: string }
    /**
     * Something a sub-agent said or did. `parent` is the tool call that started it, which
     * tells one sub-agent from another.
     */
    | { type: "sub"; parent: string; event: AgentEvent }
    | { type: "other"; raw: unknown };

/** How far along something an agent left running is. */
export type BackgroundState = "running" | "completed" | "failed" | "stopped";

/** One thing an agent left running behind its turn, as last heard of. */
export interface BackgroundTask {
    session_id: number;
    session_title: string;
    /** The agent's own id for it, which is how it is stopped. */
    task_id: string;
    description: string;
    /** What kind of thing it is, as the agent names it: `local_bash`, `local_agent`. */
    kind: string;
    state: BackgroundState;
    /** The agent's own line about how it ended. */
    summary: string | null;
}

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
    /** How much the terminal had printed before this piece, as the core counts it. */
    offset: number;
}

/** A terminal as it stands when it is opened. */
export interface TerminalOpened {
    /** What the terminal has printed so far, or the end of it, to redraw it from. */
    replay: string;
    /** Where the replay ends: output with a lower `offset` is in it, any other came after. */
    end: number;
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
    /**
     * Whether the agent is asked for its fast mode: quicker answers at a higher price. Like
     * the effort, it is taken when the agent starts. Left out, it is off.
     */
    fast?: boolean;
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
    /** Whether the agent is asked for its fast mode, where it has one. Left out, it is off. */
    fast?: boolean;
}

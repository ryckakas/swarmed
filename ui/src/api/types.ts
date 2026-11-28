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
    /**
     * The session this one works for, if any: the one whose agent started it, or one the user
     * chose, in any task of the same repository.
     */
    parent: number | null;
    /**
     * Whether another session's agent started it in its own worktree, not the user: its first
     * message is that agent's words, and its turns are reported to that session.
     */
    spawned: boolean;
    /** What the session is for, by the name of its role, such as `planner`; `null` for none. */
    role: string | null;
    /** Whether the agent is asked for its fast mode, where it has one. */
    fast: boolean;
    /** The mark an agent or the user put on it, if any. */
    mark: Mark | null;
    /**
     * Whether the message it was made with waits for its task's setup: it goes once that
     * finishes well, or when the user starts it anyway.
     */
    waits_for_setup: boolean;
    /** Whether the user pinned it above its siblings that are not. A closed session keeps its pin. */
    pinned: boolean;
    /**
     * When it goes on by itself, the usage limit that stopped it reset, in seconds since 1970:
     * a message is sent to it then. `null` for not.
     */
    continue_at: number | null;
    /**
     * What its agent has running in the background now, sub-agents and commands apart. Not a
     * status: the session stays idle, and takes a message.
     */
    background: BackgroundCount;
    /**
     * The colour the user highlighted it in, where it is; `null` for none. Writing to it
     * leaves it, and so does closing it; a fork starts without one.
     */
    highlight: MarkColour | null;
    /**
     * Until when the user snoozed it, in seconds since 1970: out of the sidebar's list and the
     * inbox until then, its agent not stopped. `null` for not snoozed. Writing to it, a
     * permission request or a question, an error and closing it take it off; a turn that ends
     * well does not, and a fork starts without one.
     */
    snoozed_until: number | null;
}

/**
 * A session that works for another, directly or through others, as the core lists those under
 * one (`Descendant`).
 */
export interface Descendant {
    session: Session;
    /** How far below: 1 for one that works for it, 2 for one that works for such a one. */
    depth: number;
    closed: boolean;
}

/** How many things a session's agent has running in the background now, by kind. */
export interface BackgroundCount {
    /** Sub-agents at work. */
    agents: number;
    /** Everything else left running: commands, and shells an agent left open. */
    commands: number;
}

/** The colours a mark or a highlight can have. Each theme draws its own. */
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
    /** Why its last setup did not finish well, in words; `null` once one did, or before any ended. */
    setup_reason: string | null;
    /** How its setup stands while it is not over; `null` once it is, well or not. */
    setting_up: SettingUp | null;
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
    /** Whether the user pinned it above its repository's tasks that are not. */
    pinned: boolean;
    /** The pull request of the branch its worktree is on, as the core last read it; `null` for none. */
    pull: Pull | null;
    /** When it was put away, in seconds since 1970; `null` while it is not. */
    done_at: number | null;
    /** Why it was put away, while it is. */
    done_why: DoneWhy | null;
    /** The number of the repository's GitHub issue it was started for; `null` for none. */
    issue: number | null;
    /** Whether the task's own Chrome runs, here or in another Swarmed. */
    browser: boolean;
    /** The colour the user highlighted it in, where it is; `null` for none. */
    highlight: MarkColour | null;
}

/** Why a task was put away: by the user, or by their rule for merged or idle tasks. */
export type DoneWhy = "hand" | "merged" | "idle";

/**
 * Where a task's setup is: making the worktree ready (a snapshot put in place), waiting for a
 * place among the repository's setups (`max_setups`), its `setup` hook, keeping a snapshot of
 * what it made, starting its stack.
 */
export type SetupStep = "preparing" | "waiting" | "hook" | "keeping" | "stack";

/** How a task's setup stands while it is not over. */
export type SettingUp =
    /** This Swarmed sets it up. `began` is in milliseconds since 1970. */
    | { state: "here"; step: SetupStep; began: number; latest: string | null }
    /** Another Swarmed on the same data sets it up. */
    | { state: "elsewhere" }
    /** It never finished, and nothing sets it up: the Swarmed that did ended first. */
    | { state: "interrupted" };

/** Why a setup was stopped. */
export type SetupStop = "removed" | "asked" | "user";

/** How a setup ended. */
export type SetupEnd =
    | { outcome: "done" }
    | { outcome: "failed"; reason: string }
    | { outcome: "stopped"; why: SetupStop; reason: string };

/** What a task's setup has come to, as it goes. */
export interface SetupProgress {
    task_id: number;
    step: SetupStep;
    /** When this setup began, in milliseconds since 1970: it tells one run from the next. */
    began: number;
    /** What it printed since the last report, each line ending in a newline. */
    output: string;
    /** Where in its log `output` starts and ends, in bytes. */
    from: number;
    to: number;
    /** How it ended, on its last report. */
    ended: SetupEnd | null;
}

/** The end of a task's setup log, from the start of a line. */
export interface SetupLog {
    text: string;
    /** Where in the log `text` ends, in bytes: what is published from there on comes after it. */
    end: number;
}

/** A task just started from a prompt, and its first session, whose prompt waits for the setup. */
export interface TaskStarted {
    task: Omit<
        Task,
        "sessions" | "running" | "url" | "urls" | "has_note" | "closed" | "stack" | "setting_up"
    >;
    session: Session;
}

/** How a task is started from a prompt. */
export interface TaskStart {
    /** An existing branch to check out instead of making one. */
    branch: string | null;
    environment: EnvironmentChoice;
    setup: SessionSetup;
    /** The first session's title, if a model named the task. */
    title: string | null;
    prompt: string;
    files: string[];
    /** The sessions the prompt mentions, by id: each one the new session could read. */
    sessions: number[];
    /** The pull requests and issues of the repository the prompt mentions, by number: each read first. */
    github: number[];
    /** Images pasted into the prompt, each in base64: the session's own once it is made. */
    images: string[];
    /** The number of the repository's GitHub issue the task is for, which it records. */
    issue: number | null;
}

/** Whose issues a list holds: everyone's, or those assigned to, opened by or mentioning the user. */
export type IssueWho = "anyone" | "assigned" | "authored" | "mentioned";

/** Whether an issue is open, as GitHub says. */
export type IssueState = "open" | "closed";

/** Which of a repository's issues to list. */
export interface IssueFilter {
    who: IssueWho;
    state: IssueState;
    /** Words to look for, in GitHub's search syntax; empty for none. */
    search: string;
    /** How many at most, up to 200. */
    limit: number;
}

/** One of a repository's issues, as a list shows it. Anyone can write one: nothing in it is run. */
export interface Issue {
    number: number;
    title: string;
    state: IssueState;
    /** Where it is on GitHub. */
    url: string;
    /** The names of its labels. */
    labels: string[];
    /** The logins of the people it is assigned to. */
    assignees: string[];
    /** The login of whoever opened it; empty for an account that is gone. */
    author: string;
    /** When it last changed, as GitHub writes it. */
    updated_at: string;
}

/** One comment on an issue. */
export interface IssueComment {
    /** The login of whoever wrote it; empty for an account that is gone. */
    author: string;
    /** What it says, in Markdown; empty for one GitHub hides. */
    body: string;
    created_at: string;
    /** Why GitHub hides it, such as `spam` or `off topic`, if it does; its words are left out. */
    hidden: string | null;
}

/** An issue read whole: what it says, and its comments, oldest first. */
export interface IssueDetail extends Issue {
    created_at: string;
    /** What it says, in Markdown. */
    body: string;
    comments: IssueComment[];
}

/** A repository's issues, the latest changed first, or why none could be listed. */
export interface IssueList {
    issues: Issue[];
    /** Why there are none to show: no `gh`, not on GitHub, no sign-in, in `gh`'s words. */
    unavailable: string | null;
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
    /**
     * Whether the files its environment's dependencies are pinned in have changed on the base
     * branch since: the next task started from the base takes its place.
     */
    behind: boolean;
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
    /**
     * The branch the worktree is on: the task's own unless something switched it, and null
     * when it is on no branch at all. The figures above are that branch's.
     */
    branch: string | null;
    /**
     * Every branch the worktree has been seen on that is still there, the task's own first.
     * More than one means something switched it.
     */
    branches: string[];
    /** Whether a rebase is under way, which leaves the worktree on no branch until it is over. */
    rebasing: boolean;
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
    /** Whether its branch conflicts with its base, as GitHub last worked it out. */
    conflicts: boolean;
    /** Its head commit, which a merge names so that only what was read is merged. */
    head: string;
    /** The branch it goes into. */
    base: string;
    /** The method GitHub merges it with once its requirements are met, while auto-merge is on. */
    auto_merge: MergeMethod | null;
    /**
     * Whether GitHub would merge it now: `clean`, `unstable` (checks no rule requires did not
     * pass), `has_hooks`, `blocked` (a requirement is not met), `behind`, `dirty` or `draft`;
     * `null` while GitHub has not worked it out.
     */
    merge_state: string | null;
    /** When it was merged, as GitHub writes it (`2026-11-20T10:00:00Z`), if it was. */
    merged_at: string | null;
}

/** How a pull request's commits go into its base. */
export type MergeMethod = "merge" | "squash" | "rebase";

/**
 * When, and past what, a pull request is merged: now unless a check failed, now although
 * checks failed, now past the repository's requirements as its admin, or by GitHub's own
 * auto-merge once they are met.
 */
export type MergeWhen = "now" | "force" | "bypass" | "auto";

/** What a repository allows for merging, and what the user may do there. */
export interface MergeRules {
    /** The methods it allows, in the order GitHub lists them. */
    methods: MergeMethod[];
    /** The method a merge starts on: the last used there, the user's own default, or the first. */
    method: MergeMethod | null;
    /** Whether it allows auto-merge. */
    auto_merge: boolean;
    /** Whether the user administers it, which lets them merge past its requirements. */
    admin: boolean;
}

/** What came of a merge: merged, taken by a merge queue, auto-merge on or off, or not known yet. */
export type MergeOutcome = "merged" | "queued" | "auto_merge" | "auto_merge_off" | "asked";

/** What came of a merge, and the pull request as read after it. */
export interface MergeReport {
    outcome: MergeOutcome;
    pull: Pull | null;
}

/** What changed on a task's pull request: who reviewed or commented, never what they wrote. */
export type PullChange =
    | { kind: "checks_failed"; checks: string[] }
    | { kind: "checks_passed" }
    | { kind: "changes_requested"; by: string[] }
    | { kind: "approved"; by: string[] }
    | { kind: "reviewed"; by: string[] }
    | { kind: "commented"; by: string[] }
    | { kind: "conflicts" }
    | { kind: "merged" }
    | { kind: "closed" };

/**
 * Which environment a new task is set up in: the project's default one (or its own setup when
 * it has none), the project's own setup, or the one of this name.
 */
export type EnvironmentChoice = "default" | "own" | { named: string };

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
    /**
     * How many of the project's stacks may run for one of this environment's tasks to start its
     * own. Left out, the project's own `max_stacks`.
     */
    max_stacks?: number;
    /**
     * How many of the project's tasks may run their setup at once for one of this environment's
     * tasks to run its own; 0 is no limit. Left out, the project's own `max_setups`.
     */
    max_setups?: number;
    /**
     * The tracked files its dependencies are pinned in: once one changes on the base branch, the
     * next task started from it keeps a newer snapshot. Left out, the project's `snapshot_keys`.
     */
    snapshot_keys?: string[];
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

/**
 * One turn of a session whose checkpoints are kept: a message sent to its agent, and what came
 * of it. The core keeps the latest 50 of each session.
 */
export interface Turn {
    /** The message that began it, counting from 0 every message sent into the session. */
    message: number;
    /** The start of that message. */
    words: string;
    /** When the message went, in milliseconds since 1970. */
    at_ms: number;
    /** Whether its end was recorded; without one, its changes are counted up to now. */
    ended: boolean;
    /** The titles of the task's other sessions at work while it was: their changes are in it too. */
    beside: string[];
}

/** A file git does not track that was too large to keep in a checkpoint. */
export interface SkippedFile {
    path: string;
    bytes: number;
}

/** What taking a session back to before one of its messages would do now. */
export interface RewindCheck {
    /** Why it cannot be taken back now, if it cannot. */
    refused: string | null;
    /** Why its files cannot go back with it now, if they cannot. */
    files_refused: string | null;
    /** Whether the agent resumes its own conversation there; otherwise it starts a new one. */
    resumes: boolean;
    /** What taking the files back would change, from now to before the message. */
    files: FileChange[];
    /** Commits made since the message, which stay on the branch; `null` when not known. */
    commits_since: number | null;
}

/** What came of taking a session back. */
export interface RewindDone {
    to: number;
    files: boolean;
    resumes: boolean;
    commits_since: number | null;
}

/** What one turn changed in the worktree. */
export interface TurnChanges {
    files: FileChange[];
    /** Files left out at either end: what became of them is not known. */
    skipped: SkippedFile[];
    /** Whether the turn's end was recorded; without one, this is up to now. */
    ended: boolean;
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

/** The levels of the sidebar a custom layout can set apart, by the core's names for them. */
export type SidebarLevelName = "repository" | "task" | "session" | "sub_agent";

/** A sidebar level's text colour and the background of its rows in one form, each `#rrggbb`. */
export interface LevelColours {
    colour?: string;
    background?: string;
}

/** How one level of the sidebar is drawn: its text size in pixels, its weight and colours. */
export interface SidebarLevel {
    size?: number;
    weight?: number;
    dark?: LevelColours;
    light?: LevelColours;
}

/**
 * The user's own stylesheet, `style.css` in the data directory, as the core reads it. The window
 * applies only what the user saved in Settings or accepted when told the file changed.
 */
export interface Stylesheet {
    /** Whether the user turned it on (`user_stylesheet`). */
    on: boolean;
    /** Whether this run leaves it out all the same: ⌥ was held as Swarmed started. */
    left_out: boolean;
    /** What the window applies now; empty while off, left out or without a file. */
    applied: string;
    /** The text the user last saved or accepted; empty for none. */
    accepted: string;
    /** What the file holds; `null` without a file, or one too large to read. */
    file: string | null;
    /** Whether the file is there but too large to be read: nothing of it is applied. */
    too_large: boolean;
    /** When the file last changed, in seconds since 1970. */
    changed_at: number | null;
    /** Whether the file holds what the user has not accepted: a change on disk, waiting. */
    waiting: boolean;
}

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
    sidebar?: Partial<Record<SidebarLevelName, SidebarLevel>>;
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
    | "duration"
    | "tokens";

/**
 * How the agents' tool calls show in a conversation: each on its own line, the ones that follow
 * each other folded into one, or none. Requests, questions, plans and views always show.
 */
export type ToolCalls = "all" | "folded" | "hidden";

/**
 * Which levels of the account's usage the notice at the top of the window says: every one
 * (50%, 75%, 90% used, billed, stopped), from 90% used, or only requests stopped.
 */
export type LimitWarnings = "frequent" | "rare" | "none";

/**
 * Which formulas in what agents write are drawn as math: none; `$$…$$`, `\(…\)`, `\[…\]` and
 * fenced `math` blocks, while a single `$` stays a dollar sign; or those and `$…$` where it reads
 * as math.
 */
export type MathShown = "off" | "standard" | "with_single_dollars";

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
    /**
     * Whether the window applies the user's own stylesheet after its own: what they last saved
     * or accepted. Agents cannot change it.
     */
    user_stylesheet: boolean;
    /** How much a new session's agent may do without asking, unless chosen otherwise. */
    default_supervision: Supervision;
    /**
     * How many open sessions may work for one session before its agent starts no more, from 1
     * to 20 (`WORKER_LIMITS`). Six unless changed. Agents cannot change it.
     */
    max_workers: number;
    /**
     * The model and effort the sessions of a role start on where nothing else was chosen, by
     * the role's name: whoever starts them, the user or an agent. Left out while there are
     * none. Agents cannot change it.
     */
    role_defaults?: Record<string, RoleDefault>;
    /** Whether agents may answer with small HTML views, and those are shown. */
    html_views: boolean;
    /** How the agents' tool calls show in a conversation. */
    tool_calls: ToolCalls;
    /**
     * Whether a conversation draws a line where each turn that showed something ends. A turn
     * that failed or was stopped keeps its line either way.
     */
    turn_end_lines: boolean;
    /** Which formulas in what agents write are drawn as math. */
    math: MathShown;
    /** Which levels of the account's usage the notice at the top of the window says. */
    limit_warnings: LimitWarnings;
    /** Whether a session a usage limit stopped goes on by itself when it resets, unasked. */
    continue_after_limit: boolean;
    /** Whether the system notifies when a session needs you, finishes or fails out of sight. */
    notifications: boolean;
    /** The built-in pieces of the status line under the message box, in order. */
    status_items: StatusItem[];
    /** A command whose first line of output replaces the built-in pieces. Empty for none. */
    status_command: string;
    /** The user's own rules, given to every agent whatever the model. Empty for none. */
    rules: string;
    /** How Swarmed gets its new versions. */
    updates: UpdateMode;
    /**
     * What a new task's branch starts with, joined to the task's name by one `/`, where the
     * repository keeps the default pattern. Kept without slashes at its ends; empty for the
     * user's login.
     */
    branch_prefix: string;
    /** Whether a task whose pull request merged is put away by itself, some days after. */
    put_away_merged: boolean;
    /** How many days after the merge, from 0 to 365. */
    put_away_merged_days: number;
    /** Whether a task none of whose sessions recorded anything for some days is put away. */
    put_away_idle: boolean;
    /** How many days without anything in its sessions, from 1 to 365. */
    put_away_idle_days: number;
    /** Whether putting a task away also closes its sessions. */
    put_away_closes_sessions: boolean;
    /**
     * How many days after it was put away a task is removed by itself, as a forced Remove…
     * does, from 0 to 365; 0 never removes.
     */
    put_away_remove_days: number;
    /** The hour, 0 to 23, a snooze until the morning wakes at: tomorrow's, or next Monday's. */
    snooze_morning_hour: number;
    /** The hour, 0 to 23, a snooze until this evening wakes at. */
    snooze_evening_hour: number;
    /** The program a task's browser runs; empty for the first of Chrome, Chromium, Edge, Brave found. */
    browser_program: string;
    /** Minutes a task's browser may sit unused before it is stopped, up to 1440; 0 for never. */
    browser_idle_minutes: number;
    /** Where a task's browser is shown: drawn in the pane, or as a Chrome window of its own. */
    browser_display: BrowserDisplay;
}

/**
 * Where a task's browser is shown. Either way it is the same Chrome on the task's profile, and
 * agents' browser tools act in it.
 */
export type BrowserDisplay = "pane" | "window";

/**
 * How Swarmed gets its new versions: by itself, at launch and every few hours, installed as it
 * quits; or only when the About view looks.
 */
export type UpdateMode = "automatic" | "when_asked";

/** Where updating Swarmed stands. */
export type UpdateState =
    | { state: "unknown" }
    | { state: "checking" }
    | { state: "up_to_date" }
    /** Out, and not downloaded. `failed` is why it did not go in when Swarmed last quit. */
    | { state: "available"; version: string; failed?: string }
    /** Coming in: `received` bytes so far, of `total` if the server said. */
    | { state: "downloading"; version: string; received: number; total: number | null }
    /** Downloaded and its signature checked: it goes in when Swarmed quits. */
    | { state: "ready"; version: string }
    | { state: "failed"; reason: string };

export interface Project {
    id: number;
    name: string;
    repo_path: string;
    /**
     * Whether this is where the chats are kept: sessions with no repository, each chat a task
     * in a folder of its own. Its name is `CHATS`, which no repository can have.
     */
    chats: boolean;
    tasks: Task[];
    /** Whether the user keeps a note on the repository. */
    has_note: boolean;
    /** The tasks handed to cloud agents from the repository, newest first, as last heard of. */
    cloud: CloudTask[];
    /** Whether the user pinned it above the repositories that are not. */
    pinned: boolean;
}

/**
 * A session another can point its agent at with `@`: an open one the other's agent can read,
 * under the same rules as its `read_session` tool.
 */
export interface Reachable {
    id: number;
    /** Empty while it has none. */
    title: string;
    /** The task it works in; for a chat, the chat's name. */
    task: string;
    /** The repository of its task; for a chat, the home of the chats. */
    project: string;
    chat: boolean;
    /** Whether it works in the same task as the session that would point at it. */
    same_task: boolean;
    status: SessionStatus;
}

/** Whether a numbered thing on GitHub is a pull request or an issue: they share one count. */
export type GitHubKind = "pull" | "issue";

/** Where a pull request or an issue stands; only a pull request is a draft or merged. */
export type GitHubState = "open" | "draft" | "merged" | "closed";

/** A pull request or an issue of a repository, as the `#` menu offers it and a message names it. */
export interface GitHubItem {
    kind: GitHubKind;
    number: number;
    title: string;
    state: GitHubState;
    /** Where it is on GitHub, which also says the repository. */
    url: string;
    /** A pull request's branch and the one it goes into; `null` for an issue. */
    head: string | null;
    base: string | null;
    /** When it last changed, as GitHub writes it. */
    updated_at: string;
}

/** What the `#` menu offers, or why it offers nothing. */
export interface GitHubItems {
    /** The latest changed first. */
    items: GitHubItem[];
    /** Why there are none: no `gh`, not on GitHub, no sign-in, no such number, in `gh`'s words. */
    unavailable: string | null;
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
    /** Whether Swarmed put it there by itself, when the usage limit that stopped the session reset. */
    at_reset?: boolean;
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

/**
 * Where a built-in MCP server stands: not installed, older than Swarmed needs, off, turned on
 * but not given to sessions while another setup of it is still about, and on.
 */
export type BuiltinMcpState = "missing" | "outdated" | "off" | "waiting" | "on";

/** A server Swarmed installs and runs itself, such as repolore, as the settings show it. */
export interface BuiltinMcpView {
    name: string;
    state: BuiltinMcpState;
    /** The version installed; `null` if none is. */
    version: string | null;
    /** The oldest version Swarmed works with. */
    minimum: string;
    /** What Install and Update run, shown before they do. */
    install_command: string;
    /** The built-in's own folder, in Swarmed's data directory. */
    home: string;
    /** Whether that folder holds a setup already, so turning on needs no copy or fresh start. */
    home_ready: boolean;
    /** The user's own setup from before, offered for a copy while off and for the Trash once on. */
    existing: ExistingSetup | null;
    /** Where it is registered outside Swarmed: each holds it back while turned on. */
    registrations: BuiltinRegistration[];
    /** The old shared server, while it is in use. */
    old_server: OldServer | null;
    /** Swarmed's sessions whose agents still use the old server. */
    sessions: HeldSession[];
    /** Whether Claude Code's settings hold a `repolore session-start` hook: pointed out, never edited. */
    hook: boolean;
    /** What the last doctor found. */
    checks: DoctorCheck[];
    /** Which doctor that was: the quick one without the network, or the full one; `null` if none ran. */
    checked: "offline" | "full" | null;
    /** What Claude Code last said of the server, such as `connected`; `null` if it has not. */
    status: string | null;
    /** Whether the server's own page runs. */
    page_open: boolean;
    /** Why the last reading or step failed. */
    problem: string | null;
}

/** The user's setup from before the built-in: its folders, and whether it has an index. */
export interface ExistingSetup {
    folders: string[];
    indexed: boolean;
}

/** One place outside Swarmed where a built-in is registered, and how it is taken out. */
export interface BuiltinRegistration {
    /** What names it to `builtinMcpRemoveRegistration`. */
    key: string;
    /** The agent or app it is registered in, such as Claude Code. */
    client: string;
    scope: string;
    /** The command that removes it: shown before Swarmed runs it, or for the user to run. */
    command: string | null;
    /** Whether Swarmed runs that command; if not, it is a step for the user. */
    runs_here: boolean;
    /** What the user does to take it out, where Swarmed does not: with a command, what comes before it. */
    step: string | null;
}

/**
 * The old shared server, while it is in use: it runs or starts, or sessions are still connected
 * to it, each of which would start it again.
 */
export interface OldServer {
    /** Whether it runs or is starting: repolore says it runs before it says where. */
    running: boolean;
    /** Its process; `null` while it is stopped, or starting and not saying yet. */
    pid: number | null;
    /** How many sessions are connected to it. */
    relays: number;
}

/** A session of Swarmed's whose agent still uses the old server. */
export interface HeldSession {
    id: number;
    title: string;
    /** Whether its agent waits for a message, so restarting it loses nothing. */
    idle: boolean;
}

/** One of the doctor's checks, with what fixes it. */
export interface DoctorCheck {
    name: string;
    ok: boolean;
    warning: boolean;
    detail: string;
    fix: string | null;
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
    /** The images kept for sessions: pasted into messages, and returned by tools. */
    images_bytes: number;
    /** The checkpoints of every task's turns. */
    checkpoints_bytes: number;
    /** The tasks' browsers: their profiles and what their pages downloaded. */
    browsers_bytes: number;
    /** The built-in repolore's folder: its configuration and its index. */
    repolore_bytes: number;
    /** Every active task's worktree, largest first; `bytes` is `null` if it was not measured. */
    worktrees: { project: string; task: string; path: string; bytes: number | null }[];
    /** Removed tasks' worktrees that are not deleted yet, or could not be. */
    removed_bytes: number;
    /** Swarmed's own process with everything it started. */
    running: ProcessUsage;
    /** The share of that taken by each session whose agent is running. */
    sessions: ({ session_id: number; title: string } & ProcessUsage)[];
}

/**
 * A kind of thing a clean-up deletes: removed tasks' worktrees not deleted yet, branches of
 * removed tasks, stale worktrees, snapshots, and the checkpoints and browsers of tasks that
 * are gone.
 */
export type CleanupKind =
    | "removed"
    | "branches"
    | "stale"
    | "snapshots"
    | "checkpoints"
    | "browsers";

/** One thing a clean-up can delete, as the core lists it. */
export interface CleanupItem {
    /** What names it to `cleanupClean`: unique within its kind. */
    key: string;
    project: string;
    /** What it is: a folder's path, a branch, or an environment. */
    name: string;
    /** The task it was, or for a snapshot the task it was taken from, where known. */
    task: string | null;
    /** Why it is there, or what deleting it would lose, in words. */
    reason: string;
    /** What it takes up on disk; `null` when it is not on disk or could not be measured. */
    bytes: number | null;
    /** Since when it is there, in seconds since 1970, where known. */
    since: number | null;
    /** Whether deleting it would lose work found nowhere else: then it goes only when ticked. */
    at_risk: boolean;
}

/** What can be cleaned up, kind by kind. */
export type Cleanup = Record<CleanupKind, CleanupItem[]>;

/** What a clean-up did: the keys it deleted, and what it did not, with why. */
export interface Cleaned {
    deleted: string[];
    kept: { key: string; name: string; reason: string }[];
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

/** One of the plan's usage windows, as the newest report any Claude Code session heard gave it. */
export interface UsageWindow {
    /**
     * What tells it apart, as Claude Code's report after a turn names it: `five_hour`,
     * `seven_day`, or `seven_day_` and a model. A report's `seven_day_overage_included` is one
     * model's own weekly limit without its name, which the core folds into that model's window
     * once an answer has named the model.
     */
    kind: string;
    /** What it is called here, such as `5-hour` or `weekly Fable`. */
    name: string;
    /** How much of it is used, from 0 to 1. */
    used: number;
    /** When it resets, in seconds since 1970, if said. */
    resets_at: number | null;
    /** `allowed`, or Claude Code's word for how near it is or what stops the next request. */
    status: string;
    /** When Swarmed heard it, in seconds since 1970. */
    heard_at: number;
    /** Whether it came from an answer to asking, and not from a report after a turn. */
    asked: boolean;
}

/** What extra usage stood at when Claude Code was last asked, in minor units of `currency`. */
export interface ExtraUsageHeard {
    enabled: boolean;
    used: number;
    limit: number | null;
    currency: string;
    decimals: number;
    limit_reached: boolean;
    /** When Swarmed heard it, in seconds since 1970. */
    heard_at: number;
}

/** The account's usage as any Claude Code session last heard it, which every session shows. */
export interface AccountUsage {
    /** Every window heard of, each with its newest figure, by kind. */
    windows: UsageWindow[];
    extra: ExtraUsageHeard | null;
    /**
     * What the newest report after a turn that said anything of extra usage said of it. A
     * report of a window still within, while extra usage pays for another window's limit until
     * it resets, says nothing of it.
     */
    billed: { on: boolean; resets_at: number | null; heard_at: number } | null;
    /**
     * Whether extra usage can pay past a limit reached, as the newest word on it said: a report
     * after a turn or the extra usage asked for, which never overrules a report's refusal
     * (`asked` says it came from one). Not whether it does: a limit within needs nothing paid.
     */
    pays: { can: boolean; heard_at: number; asked: boolean } | null;
}

/**
 * When a question for the account's usage goes to Claude Code. Claude Code answers from its own
 * copy of the figures for a minute after it fetched them, so while that copy is younger, the
 * question waits until Claude Code would fetch them again.
 */
export interface UsageAsk {
    /** When it goes, in milliseconds since 1970, if it waits; `null` when it went at once. */
    waits_until: number | null;
    /** When the copy it waits out was fetched, in milliseconds since 1970. */
    copy_fetched_at: number | null;
}

/** One part of what fills an agent's context window. */
export interface ContextCategory {
    name: string;
    tokens: number;
    /** `used`, `free`, `buffer` or `deferred`, in the agent's words. */
    kind: string;
}

/**
 * What Swarmed does with a command when it is sent: send it on, show a screen, ask about a
 * setting, do it itself with nothing to ask (`own`, such as Remote Control), open a choice of
 * its own in the window (`choice`, such as `/rewind`), or refuse it.
 */
export type CommandKind = "prompt" | "screen" | "setting" | "own" | "choice" | "terminal";

/**
 * How Remote Control of a session stands: asked for, reachable from claude.ai, reconnecting,
 * waiting for its agent to run again, turned off, or failed until it is turned on again.
 */
export type RemoteState = "starting" | "on" | "reconnecting" | "paused" | "off" | "failed";

/** A command or skill a message can call by starting with `/`. */
export interface SlashCommand {
    /** The name, without its slash. */
    name: string;
    kind: CommandKind;
    /** For one Swarmed does not send on, what to do instead; empty for any other. */
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

/** What an MCP server declared its app may reach, as it declared it. */
export interface AppCsp {
    connect: string[];
    resources: string[];
    frames: string[];
    base_uris: string[];
}

/** An app made ready for a frame: its address's id, and what its policy lets it reach. */
export interface AppShown {
    id: string;
    policy: string;
    /** Every origin it may reach, which the user is shown before it loads. */
    reaches: string[];
    /** What its server declared and the policy left out, each with why. */
    refused: string[];
}

export type AgentEvent =
    /**
     * What the user wrote; `remote` when typed on claude.ai or in the Claude app, `at_reset`
     * when Swarmed sent it by itself as the usage limit that stopped the session reset, and
     * `from_session` the session whose agent left it (`send_message`), absent for the user's
     * own. Only the user's own wakes a snoozed session.
     */
    | {
          type: "user_message";
          text: string;
          remote?: boolean;
          at_reset?: boolean;
          from_session?: number;
      }
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
          /** What the agent says it can do beyond the basics; absent from older records. */
          capabilities?: string[];
      }
    | { type: "text"; text: string }
    /** A piece of a reply still being written; the whole reply follows as `text`. */
    | { type: "text_delta"; text: string }
    /**
     * The tokens the turn under way has used so far, and which way they go now; published as
     * they change and never stored. A sub-agent's are its own, inside `sub`.
     */
    | { type: "turn_tokens"; tokens: Tokens; phase: TokenPhase }
    | { type: "tool_use"; id: string; name: string; input: unknown }
    | {
          type: "tool_result";
          tool_use_id: string;
          /** The result as text; `[image]` where it held an image, or why one was not kept. */
          content: string;
          is_error: boolean;
          /**
           * The images it returned that were kept, in order, as files of the session's own:
           * read with `sessionImage`. Absent where it returned none, and from older results.
           */
          images?: string[];
      }
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
    /**
     * Swarmed's configuration was changed, from a session or, with session id 0 and the kind
     * `file`, in one of its files on disk: what was read of it is out of date.
     */
    | { type: "config_changed"; kind: string }
    /** The session got a mark, or lost the one it had when `colour` is `null`. */
    | { type: "marked"; colour: MarkColour | null; label: string }
    /** The session was closed, by the user or by the session that started it. */
    | { type: "closed" }
    /** The session was deleted for good, with everything said in it. */
    | { type: "deleted" }
    /**
     * Repositories, tasks, sessions or cloud tasks were put in another order. It belongs to no
     * session: it comes with session id 0, and the tree is read again.
     */
    | { type: "reordered"; kind: string }
    /**
     * Another Swarmed process on the same data, such as the command line, changed repositories,
     * tasks or sessions, or started or stopped a stack. It belongs to no session: it comes with
     * session id 0, and the tree is read again.
     */
    | { type: "tree_changed" }
    /**
     * What a Claude Code session heard changed the account's usage. It belongs to no session:
     * it comes with session id 0, and every session shows it.
     */
    | { type: "account_usage"; usage: AccountUsage }
    /**
     * How a task's setup goes: where it is, what it printed, how it ended. It belongs to no
     * session: it comes with session id 0, and is never stored.
     */
    | { type: "setup"; progress: SetupProgress }
    /**
     * A task put away for `days` days was removed by itself, as a forced Remove… does; `notes`
     * are what the removal passed over. It belongs to no session: it comes with session id 0,
     * and is never stored. The tree is read again on the `tree_changed` that follows it.
     */
    | { type: "task_removed"; project: string; task: string; days: number; notes: string[] }
    /** What stands before this is a copy of what was said in the session forked from. */
    | { type: "forked"; from: number }
    /**
     * Where Claude Code's own record of the conversation stood as a turn ended, at which a
     * rewind resumes it. Stored, never shown.
     */
    | { type: "resume_point"; uuid: string }
    /**
     * The session was taken back to before its message number `to` (counting from 0 every
     * message sent into it): that message and what followed, up to here, no longer count.
     * `files` says whether the worktree's files went back too; `resume_at` is where Claude Code
     * resumes, or `null` when the agent starts a new conversation told what was said;
     * `commits_since` how many commits made since stay on the branch, if that is known.
     */
    | {
          type: "rewound";
          to: number;
          files?: boolean;
          resume_at?: string | null;
          commits_since?: number | null;
      }
    /** The files a rewind took back were put back as they were before it. */
    | { type: "files_put_back"; to: number }
    /** HTML the agent showed through Swarmed's own tool, handed over by the tool itself. */
    | { type: "view"; html: string }
    /**
     * The app an MCP server attached to the tool of a call (MCP Apps), read once the call had
     * its result: kept in the session's own folder, `document`, or not kept, `note` saying why.
     */
    | {
          type: "app";
          tool_use_id: string;
          server: string;
          tool: string;
          uri: string;
          document: string | null;
          note: string | null;
          csp: AppCsp;
          /** The devices and the like it asked for, which are never granted. */
          permissions: string[];
          prefers_border: boolean | null;
          /** The tools of its own server it may call. */
          callable: string[];
          /** The call's result as an app is handed it (MCP's `CallToolResult`), if kept. */
          result: unknown;
      }
    /** What the agent says it actually runs with, defaults resolved. */
    | { type: "applied"; model: string; effort: string | null }
    /**
     * The session goes on by itself when the usage limit that stopped it resets, at `at` in
     * seconds since 1970, or no longer does (`null`). A message of the user's takes it off too.
     */
    | { type: "continuation"; at: number | null }
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
    /**
     * How Remote Control stands: whether the session can be reached from claude.ai and the
     * Claude app, and where (`url`), with Claude Code's words for a failure or a reconnection.
     */
    | {
          type: "remote_control";
          state: RemoteState;
          url: string | null;
          remote_id: string | null;
          detail: string | null;
      }
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
          /**
           * The tokens the session's own agent used in the turn, its sub-agents' left out.
           * Absent where the agent did not say, and from turns recorded before it was kept.
           */
          tokens?: Tokens;
      }
    | { type: "exited"; code: number | null; stderr: string }
    | {
          type: "background";
          task_id: string;
          description: string;
          kind: string;
          state: BackgroundState;
          summary: string | null;
          /** The tool call that started it, for a sub-agent; `null` in older conversations. */
          tool_use_id: string | null;
          /** Where a command's output is written, if the agent said. */
          output_file: string | null;
          /**
           * Set when it ran in the foreground first, its call waiting on it, and was then moved
           * to the background. Absent otherwise.
           */
          moved?: boolean;
          /**
           * Set when the agent offers no way to stop it, such as a shell OpenCode left running.
           * Absent otherwise. The Background list reads `BackgroundTask.stoppable` instead.
           */
          unstoppable?: boolean;
      }
    /** One of the agent's own screens, such as `/usage`, with the contents the agent gave. */
    | { type: "screen"; command: string; title: string; note: string; sections: ScreenSection[] }
    /** A command that changes a setting, such as `/model`, puts its choices to the user. */
    | { type: "command_asked"; request_id: string; command: string; questions: AskedQuestion[] }
    /** The user answered such a question or put it aside; `said` is what came of it. */
    | { type: "command_answered"; request_id: string; said: string }
    /**
     * A command that only opens a screen in the agent's own terminal was sent, and the agent
     * said it has no screen to open it in. `command` is as it was typed, without its slash.
     */
    | { type: "terminal_only"; command: string }
    /**
     * The task's worktree was found on another branch than before (`null`: no branch): seen in
     * the middle of a turn, or, when it moved between turns, told to the agent with the user's
     * message.
     */
    | { type: "worktree_moved"; from: string | null; to: string | null; between_turns: boolean }
    /**
     * Whom session `child` works for changed, by the user's choice: recorded in it and in the
     * sessions it worked for and works for now. `null` is nobody.
     */
    | { type: "parent_changed"; child: number; parent: number | null; previous: number | null }
    /** The session's role changed, once something was said in it: its rules apply from here on. */
    | { type: "role_changed"; role: string | null; previous: string | null }
    /** The task's pull request changed in a way worth telling, recorded in one of its sessions. */
    | { type: "pull_changed"; number: number; url: string; change: PullChange }
    /** A task's pull request as just read; with session id 0, never stored. */
    | { type: "pull_read"; task_id: number; pull: Pull | null }
    /**
     * Something a sub-agent said or did. `parent` is the tool call that started it, which
     * tells one sub-agent from another.
     */
    | { type: "sub"; parent: string; event: AgentEvent }
    | { type: "other"; raw: unknown };

/** How far along something an agent left running is. */
export type BackgroundState = "running" | "completed" | "failed" | "stopped";

/** The tokens a turn used: what the model had to read anew, and what it wrote. */
export interface Tokens {
    /** What each request held that the model read anew: all of it but what came from the cache. */
    input: number;
    /** What it wrote: replies, thinking and tool calls. */
    output: number;
}

/**
 * Which way a running turn's tokens go: a request out and being read, its answer coming in,
 * or neither, while a tool runs.
 */
export type TokenPhase = "prompt" | "response" | "between";

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
    /** The tool call that started it, for a sub-agent: what opens its conversation. */
    tool_use_id: string | null;
    /** Whether what it printed can be read, with `sessionBackgroundOutput`. */
    output: boolean;
    /** Whether it can be stopped: not what its agent offers no way to stop, such as an OpenCode shell. */
    stoppable: boolean;
}

/** What a command an agent left running printed, or the part of it read this time. */
export interface BackgroundOutput {
    /** What it printed, from `start` on. */
    text: string;
    /** Where `text` starts in what it printed, in bytes: past the beginning for its latest part. */
    start: number;
    /** Where `text` ends: where to read on from. */
    end: number;
}

export interface SessionEvent {
    session_id: number;
    /** The session's status after this event, if the event changed it. */
    status: SessionStatus | null;
    /**
     * Whether the session is unread after this event, if the event changed that; `null` if it
     * did not, as for a turn that ends while sub-agents work on in the background. The core
     * always says; events the window makes up itself (the sample data) leave it out.
     */
    unread?: boolean | null;
    /**
     * What the session's agent has running in the background after this event, if the event
     * could change that: a report of something left running, or the agent's end. The core
     * always says; events the window makes up itself leave it out.
     */
    background?: BackgroundCount | null;
    event: AgentEvent;
}

/** A piece of what a session's terminal printed. */
export interface TerminalOutput {
    session_id: number;
    /** The text, with the escape sequences a terminal emulator expects. */
    data: string;
    /** How much the terminal had printed before this piece, as the core counts it. */
    offset: number;
    /** What runs in it has ended: `data` is empty, and nothing follows. */
    ended: boolean;
    /** Which run of the terminal printed this, as `TerminalOpened.run` names it. */
    run: number;
}

/** A terminal as it stands when it is opened. */
export interface TerminalOpened {
    /** What the terminal has printed so far, or the end of it, to redraw it from. */
    replay: string;
    /** Where the replay ends: output with a lower `offset` is in it, any other came after. */
    end: number;
    /** Which run of the terminal this is: only output of the same run belongs to it. */
    run: number;
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
    /**
     * What a session that leaves the model open runs on, where Swarmed decides that rather than
     * the agent: the first of `models`, for an agent picked from OpenRouter. `null` otherwise.
     */
    default_model: string | null;
    /**
     * Whether a message written while it works can be taken into the turn under way, at its
     * next step, without stopping it: Claude Code's way.
     */
    hears_mid_turn: boolean;
    /**
     * Whether a command or sub-agent that a tool call waits on can be sent to the background,
     * so that the call returns and the turn goes on: Claude Code's Ctrl+B.
     */
    sends_to_background: boolean;
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
    /** The name of the session's role; left out or `null` for none. */
    role?: string | null;
}

/**
 * A role's own model and effort (`RoleDefault`); either left out is the agent's own, or that of
 * the agent that starts the session. A model the session's agent does not offer is passed over.
 */
export interface RoleDefault {
    /** A model by the name its agent offers it under, such as `fable`. */
    model?: string | null;
    /** How hard the model thinks. */
    effort?: Effort | null;
}

/** A role a session can have, as the core keeps it (`roles.rs`). */
export interface RoleInfo {
    /** Its name, which the session keeps: `architect` and the like. */
    name: string;
    /** What it is for, in one line that follows "A planner": "plans one part inside one task…". */
    description: string;
    /** The supervision it starts in; `null` for what was chosen for the session. */
    supervision: Supervision | null;
    /**
     * The supervision it goes on in once the user approves its plan, for a role that hands the
     * plan on to sessions it starts and follows; `null` to stay in plan mode.
     */
    approved_supervision: Supervision | null;
    /** Whether its agent changes files. */
    changes_files: boolean;
    /** The roles of the sessions it may start. */
    starts: string[];
    /** The role of a session it starts without naming one. */
    child: string | null;
}

/** The browser a task's Chrome runs on, or what would install one. */
export interface BrowserSetup {
    /** Such as "Google Chrome"; `null` if none was found. */
    name: string | null;
    path: string | null;
    /** What would install one, shown before it runs; `null` where Swarmed knows no way. */
    install_command: string | null;
    /** Whether the program was set in the settings rather than found. */
    chosen: boolean;
}

/** A dialog a page opened (`alert`, `confirm`, `prompt`), which holds it until answered. */
export interface PageDialog {
    kind: string;
    message: string;
    default_prompt: string;
}

/** A page open in a task's Chrome. */
export interface BrowserTab {
    id: string;
    title: string;
    url: string;
    loading: boolean;
    can_go_back: boolean;
    can_go_forward: boolean;
    dialog: PageDialog | null;
    /** The session whose browser tools last acted in it. */
    acted_by: number | null;
}

/** Where an element was on the page's screen, in CSS pixels. */
export interface ElementBox {
    x: number;
    y: number;
    width: number;
    height: number;
}

/** One computed style of an element. */
export interface ElementStyle {
    property: string;
    value: string;
}

/**
 * An element of a page the user picked in the Browser pane, as read from Chrome and kept for
 * the session its message goes to, with its picture. A message names it by its `number`.
 */
export interface PickedElement {
    number: number;
    /** What its token and its line under the message call it: `button "Save" on /settings`. */
    label: string;
    url: string;
    tag: string;
    element_id: string | null;
    classes: string[];
    /** A selector that found it alone on the page when it was picked. */
    selector: string;
    role: string | null;
    /** Its accessible name. */
    name: string | null;
    text: string;
    html: string;
    bounds: ElementBox | null;
    styles: ElementStyle[];
    /** Its picture in the session's folder, if one could be taken. */
    picture: string | null;
}

/** What the Browser pane shows of a task's Chrome. */
export interface BrowserState {
    task_id: number;
    running: boolean;
    /** Which browser, such as "Google Chrome", once it runs here. */
    name: string | null;
    /** Where its DevTools endpoint answers: `http://127.0.0.1:<port>`. */
    endpoint: string | null;
    websocket: string | null;
    tabs: BrowserTab[];
    /** The tab the pane shows. */
    shown: string | null;
    /** Where what its pages download goes. */
    downloads: string;
    /** Whether it runs with a window of its own, which Chrome draws and the pane does not. */
    windowed: boolean;
}

export type ConsoleLevel = "debug" | "info" | "warning" | "error";

/** One line a page logged, or an error it did not catch. */
export interface ConsoleLine {
    /** Its place among every line of the task's Chrome: higher is later. */
    n: number;
    level: ConsoleLevel;
    text: string;
    /** Where it was said, as `address:line`. */
    source: string | null;
    /** When, in milliseconds since 1970. */
    at: number;
}

/** What the tasks' browsers show, as the core publishes it. */
export type BrowserEvent =
    | {
          kind: "frame";
          task_id: number;
          tab: string;
          /** A JPEG, in base64. */
          data: string;
          /** The page's size, in CSS pixels. */
          width: number;
          height: number;
      }
    | ({ kind: "state" } & BrowserState)
    | { kind: "console"; task_id: number; tab: string; line: ConsoleLine }
    /** A session's agent asked for the browser to be shown (`show_browser`), on its tab. */
    | { kind: "show"; task_id: number; session_id: number; tab: string | null };

/** The room the pane gives a page: CSS pixels, and screen pixels per CSS pixel. */
export interface Viewport {
    width: number;
    height: number;
    scale: number;
}

/** The keys held down with a click or a key. */
export interface Modifiers {
    alt: boolean;
    ctrl: boolean;
    meta: boolean;
    shift: boolean;
}

/** One thing the mouse did on the picture of a page, in the page's CSS pixels. */
export interface MouseInput {
    kind: "down" | "up" | "move" | "wheel";
    x: number;
    y: number;
    button: "none" | "left" | "middle" | "right" | "back" | "forward";
    buttons: number;
    click_count: number;
    delta_x: number;
    delta_y: number;
    modifiers: Modifiers;
}

/** One key pressed or let go on the picture of a page. */
export interface KeyInput {
    kind: "down" | "up";
    key: string;
    code: string;
    key_code: number;
    modifiers: Modifiers;
    repeat: boolean;
}

/** Where a tab is to go: to an address typed, back, forward, again, or stop loading. */
export type BrowserGo = { address: string } | "back" | "forward" | "reload" | "stop";

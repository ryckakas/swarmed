use std::path::PathBuf;

/// The result type of every fallible core operation.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong in the core.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No project is registered under this name.
    #[error("project `{0}` not found")]
    ProjectNotFound(String),
    /// A project with this name is already registered.
    #[error("project `{0}` already exists")]
    ProjectExists(String),
    /// The project has no active task with this name.
    #[error("task `{task}` not found in project `{project}`")]
    TaskNotFound {
        /// The project that was searched.
        project: String,
        /// The task name that was asked for.
        task: String,
    },
    /// The project has no environment with this name.
    #[error("project `{project}` has no environment `{environment}`")]
    EnvironmentNotFound {
        /// The project that was searched.
        project: String,
        /// The environment that was asked for.
        environment: String,
    },
    /// A name that cannot be an environment variable's.
    #[error("`{0}` cannot be the name of a variable: use letters, digits and underscores")]
    InvalidVariable(String),
    /// The project already has an active task with this name.
    #[error("task `{task}` already exists in project `{project}`")]
    TaskExists {
        /// The project the task belongs to.
        project: String,
        /// The name that is taken.
        task: String,
    },
    /// A project or task name outside the allowed character set.
    #[error("`{0}` is not a valid name: use lowercase letters, digits, `-`, `_` and `.`")]
    InvalidName(String),
    /// A project name that would have its files taken for another project's.
    #[error(
        "`{0}` cannot be a project's name: a name ending in `.actions` or `.environments` is that of a file of another project's"
    )]
    ReservedName(String),
    /// The path is not inside a git repository.
    #[error("{0} is not a git repository")]
    NotARepo(PathBuf),
    /// A `git` command exited with an error.
    #[error("`git {args}` failed: {stderr}")]
    Git {
        /// The arguments git was called with.
        args: String,
        /// What git printed to stderr.
        stderr: String,
    },
    /// A project hook exited with a non-zero status.
    #[error("the `{hook}` hook failed with {status}")]
    HookFailed {
        /// Which hook: `setup`, `start`, `stop` or `teardown`.
        hook: &'static str,
        /// The exit status as the platform describes it.
        status: String,
    },
    /// As many of the project's stacks run as its config allows.
    #[error(
        "`{project}` already has {limit} stack(s) running, which is its `max_stacks`: stop one, or raise the limit in the project's config"
    )]
    TooManyStacks {
        /// The project whose limit is reached.
        project: String,
        /// The limit, `max_stacks` in the project's config.
        limit: u32,
    },
    /// The task holds work that removing it would destroy.
    #[error("task `{task}` has {reason}; pass force to remove it anyway")]
    TaskNotClean {
        /// The task that was to be removed.
        task: String,
        /// What would be lost.
        reason: String,
    },
    /// A template names a variable that does not exist.
    #[error("unknown variable `{{{{{0}}}}}` in template")]
    UnknownVariable(String),
    /// A template opens `{{` without closing it.
    #[error("unclosed `{{{{` in template `{0}`")]
    UnclosedVariable(String),
    /// A project config file could not be parsed or holds an invalid value.
    #[error("invalid config {path}: {message}")]
    Config {
        /// The config file.
        path: PathBuf,
        /// What is wrong with it.
        message: String,
    },
    /// The editor or the browser could not be started.
    #[error("could not open {what}: {reason}")]
    OpenFailed {
        /// What was to be opened, or the command that was to open it.
        what: String,
        /// Why it did not start.
        reason: String,
    },
    /// The task's project does not say where its dev stack can be reached.
    #[error("project `{0}` has no `open_url` in its config")]
    NoOpenUrl(String),
    /// A file path that does not stay within the task's worktree.
    #[error("`{0}` is not a path inside the worktree")]
    PathOutsideWorktree(String),
    /// The user's home directory is unknown, so the default locations cannot be derived.
    #[error("could not determine the home directory")]
    NoHome,
    /// An agent could not be installed, or what is needed to set it up could not be had.
    #[error("could not set up {agent}: {reason}")]
    Install {
        /// What was being set up.
        agent: String,
        /// Why it did not work.
        reason: String,
    },
    /// An agent's program is not where its configuration says, usually because it is not
    /// installed.
    #[error(
        "the agent's program `{0}` was not found. It is probably not installed: install it \
         from Settings, under Agents, or correct its path in agents.toml"
    )]
    AgentMissing(String),
    /// Claude Code is not installed, or not where Swarmed looks for it.
    #[error(
        "Claude Code was not found (`{0}`). Swarmed runs your own Claude Code: install it from \
         Settings, under Agents, run `claude` once in a terminal to sign in, then try again. Or \
         pick another agent there"
    )]
    ClaudeMissing(String),
    /// No agent is configured under this id.
    #[error("agent `{0}` is not configured")]
    AgentNotFound(String),
    /// The place API keys are kept could not be read or written.
    #[error("the key store failed: {0}")]
    Secrets(String),
    /// A key for a provider Swarmed does not know.
    #[error("`{0}` is not an API key Swarmed can store")]
    UnknownApiKey(String),
    /// An API key with nothing in it.
    #[error("an API key cannot be empty")]
    EmptyApiKey,
    /// The user's status line script could not be run, failed, or took too long.
    #[error("status line: {0}")]
    StatusLine(String),
    /// A session's terminal could not be started or reached.
    #[error("terminal: {0}")]
    Terminal(String),
    /// The session has no terminal open.
    #[error("session {0} has no terminal open")]
    TerminalNotOpen(i64),
    /// Rules longer than rules may be.
    #[error("the rules can hold at most {0} bytes")]
    RulesTooLong(usize),
    /// A note longer than notes may be.
    #[error("a note can hold at most {0} bytes")]
    NoteTooLong(usize),
    /// Work could not be handed to a cloud agent. Says why, and what to do about it.
    #[error("{0}")]
    Cloud(String),
    /// A task's work could not be committed, pushed or put in a pull request. Says why.
    #[error("{0}")]
    Pull(String),
    /// A prompt could not be kept. Says why.
    #[error("{0}")]
    Template(String),
    /// A change to Swarmed's configuration that an agent asked for was not made. Says why.
    #[error("{0}")]
    Configure(String),
    /// A port's name is not one a hook could use.
    #[error(
        "`{0}` cannot name a port: use lowercase letters, digits and underscores, starting with a letter"
    )]
    InvalidPortName(String),
    /// A terminal's configuration cannot be used as given. Says why.
    #[error("{0}")]
    TerminalConfig(String),
    /// A custom layout cannot be drawn or read as given. Says why.
    #[error("{0}")]
    Layout(String),
    /// A mark could not be put on a session. Says why.
    #[error("{0}")]
    Mark(String),
    /// An action could not be kept or found. Says why.
    #[error("{0}")]
    Action(String),
    /// A skill could not be saved or found. Says why.
    #[error("{0}")]
    Skill(String),
    /// A snapshot could not be taken or put into a worktree. Says why.
    #[error("{0}")]
    Snapshot(String),
    /// Something one session asked about another was refused.
    #[error("{0}")]
    Peers(String),
    /// An MCP server could not be saved or found as asked.
    #[error("{0}")]
    Mcp(String),
    /// A file attached to a message is not there, or was not named by its whole path.
    #[error("{} cannot be attached: there is no such file", .0.display())]
    NoSuchAttachment(PathBuf),
    /// A message with nothing in it.
    #[error("a message cannot be empty")]
    EmptyMessage,
    /// A session title with nothing in it.
    #[error("a session title cannot be empty")]
    EmptyTitle,
    /// No session has this id.
    #[error("session {0} not found")]
    SessionNotFound(i64),
    /// The session has talked to its agent, which holds the conversation from then on.
    #[error("session {0} has begun: another agent would not know what was said")]
    SessionBegun(i64),
    /// The session was closed. Its agent is not started until it is reopened.
    #[error("session {0} is closed: reopen it first")]
    SessionClosed(i64),
    /// The session's agent has not been started, or has exited.
    #[error("session {0} is not running")]
    SessionNotRunning(i64),
    /// A stored or outgoing event could not be converted to or from JSON.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// A filesystem or process error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A database error.
    #[error(transparent)]
    Store(#[from] rusqlite::Error),
}

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
    /// The user's home directory is unknown, so the default locations cannot be derived.
    #[error("could not determine the home directory")]
    NoHome,
    /// No agent is configured under this id.
    #[error("agent `{0}` is not configured")]
    AgentNotFound(String),
    /// No session has this id.
    #[error("session {0} not found")]
    SessionNotFound(i64),
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

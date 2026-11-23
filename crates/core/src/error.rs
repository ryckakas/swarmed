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
    /// An environment that tasks still use cannot be removed.
    #[error("environment `{environment}` is used by {tasks}: remove those tasks first")]
    EnvironmentInUse {
        /// The environment that was to be removed.
        environment: String,
        /// The tasks that use it, by name.
        tasks: String,
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
    /// Swarmed's data could not be brought up to this version's form. Nothing was changed.
    #[error("Swarmed's data could not be brought up to date: {0}")]
    Migration(String),
    /// A new task's branch exists already, here or on a remote, left by an earlier task or
    /// made outside Swarmed.
    #[error(
        "a branch named {0} exists already: name the task differently, or start it from that branch"
    )]
    BranchExists(String),
    /// A new task's worktree folder is there already.
    #[error("{} exists already: name the task differently", .0.display())]
    TaskFolderExists(PathBuf),
    /// No free name for a new task was found among the ones tried.
    #[error("no free name for a task called {task} in {project}, up to {task}-99")]
    NoFreeTaskName {
        /// The project.
        project: String,
        /// The name asked for.
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
    /// As many of the project's stacks run as its config, or the task's environment, allows.
    #[error(
        "`{project}` already has {limit} stack(s) running, which is its `max_stacks`: stop one, or raise the limit in the task's environment or the project's config"
    )]
    TooManyStacks {
        /// The project whose limit is reached.
        project: String,
        /// The limit: the environment's `max_stacks`, or else the project's.
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
    /// A command whose screen Swarmed does not open in the agent's own terminal: one Swarmed
    /// does in its own way or refuses, a name that is no command's, or an agent without one.
    #[error("/{0} is not opened in Claude Code's own terminal from Swarmed")]
    NoClaudeScreen(String),
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
    /// What a command an agent left running printed cannot be read from where it is said to
    /// be: not a file this session's agent named, or not one Claude Code writes such output to.
    #[error("{0}")]
    BackgroundOutput(String),
    /// A message was to be told to an agent in the middle of its turn, which it cannot take.
    #[error(
        "{0} takes no message in the middle of its turn: send it now instead, which stops the turn"
    )]
    HearsNoMessageMidTurn(String),
    /// Work a call waits on was to be sent to the background, which only Claude Code can do.
    #[error("only Claude Code can send work to the background, and this session runs `{0}`")]
    NoSendingToBackground(String),
    /// The agent would not send work to the background, or did not say whether it did. Says
    /// why, in words for the user.
    #[error("not sent to the background: {0}")]
    NotSentToBackground(String),
    /// A port's name is not one a hook could use.
    #[error(
        "`{0}` cannot name a port: use lowercase letters, digits and underscores, starting with a letter"
    )]
    InvalidPortName(String),
    /// An environment's stack limit would let none of its stacks start.
    #[error("an environment's `max_stacks` must be at least 1; leave it out for the project's own")]
    NoRoomForAStack,
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
    /// A command of the agent's that Swarmed does not send on: it has its own way to do what
    /// the command does in a terminal, or does not leave it to the agent at all.
    #[error("/{command} is not sent. {instead}")]
    TerminalOnly {
        /// The command, without its slash.
        command: String,
        /// What to do in Swarmed instead, or why it is not done.
        instead: String,
    },
    /// An answer to a command's question that is none of what the command can be set to.
    #[error("`{answer}` is not something /{command} can be set to")]
    NotAChoice {
        /// The command, without its slash.
        command: String,
        /// What was answered.
        answer: String,
    },
    /// A session title with nothing in it.
    #[error("a session title cannot be empty")]
    EmptyTitle,
    /// No session has this id.
    #[error("session {0} not found")]
    SessionNotFound(i64),
    /// The session has talked to its agent, which holds the conversation from then on.
    #[error("session {0} has begun: another agent would not know what was said")]
    SessionBegun(i64),
    /// Only Claude Code reports the account's usage.
    #[error("only Claude Code reports usage, and this session runs `{0}`")]
    UsageNotReported(String),
    /// Only a Claude Code session can be reached from claude.ai.
    #[error("only a Claude Code session has Remote Control, and this session runs `{0}`")]
    NoRemoteControl(String),
    /// No cloud task has this id.
    #[error("cloud task {0} not found")]
    CloudTaskNotFound(i64),
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

//! The headless core of Swarmed.
//!
//! Every front end (the CLI, the desktop app) drives this crate and nothing else. It has no
//! UI dependency.
//!
//! Its API makes no promise of stability. It is published so that the `swarmed` command can be
//! installed from crates.io, and each `swarmed` requires the `swarmed-core` of its own version
//! exactly. Any release may change the API, whatever its version number: code of your own on
//! this crate should require one version exactly (`swarmed-core = "=X.Y.Z"`).

mod account_usage;
mod actions;
pub mod agent;
mod agent_models;
mod agent_setup;
mod agents;
mod analytics;
mod apps;
mod asking;
mod attach;
mod background;
mod browser;
mod changes;
mod chats;
mod checkpoints;
mod cleanup;
mod clone;
mod cloud;
mod commands;
mod config;
mod configure;
mod continuation;
mod costs;
mod effort;
mod elsewhere;
mod environments;
mod error;
mod files;
mod following;
mod fork;
mod git;
mod github_mentions;
mod handoff;
mod highlights;
pub mod hooks;
mod issues;
mod layouts;
mod leftovers;
mod lenient;
mod marks;
pub mod mcp;
mod mcp_codex;
mod mcp_opencode;
mod mcp_own;
mod mcp_plugins;
mod mcp_pool;
mod mentions;
mod merging;
mod naming;
mod notes;
mod notices;
mod open;
mod order;
mod overview;
mod pasted;
mod paths;
mod peers;
mod prefix;
mod presence;
mod publish;
mod pulls;
mod put_away;
mod queue;
mod reach;
mod remote;
mod returned;
mod rewind;
mod search;
pub mod secrets;
mod sessions;
mod settings;
mod setups;
mod shellenv;
mod skills;
mod snapshots;
mod stacks;
mod starting;
mod statusline;
mod store;
mod tasks;
mod templates;
mod termconfig;
mod terminal;
mod toolserver;
mod usage;
mod usage_copy;
mod vars;
mod views;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard};

pub use account_usage::{AccountUsage, Billed, ExtraPays, ExtraUsageHeard, UsageWindow};
pub use actions::Action;
use agent::{AgentBackend, ClaudeNamer, Namer};
pub use agent_setup::{AgentSetup, openrouter_models};
use agents::Agents;
pub use agents::{AgentInfo, CLAUDE};
pub use analytics::TurnStats;
pub use attach::{ATTACHED_FILES, Attached, with_files};
pub use background::{BackgroundCount, BackgroundOutput, BackgroundTask};
pub use browser::{
    BrowserEvent, BrowserGo, BrowserSetup, BrowserState, BrowserTab, ConsoleLevel, ConsoleLine,
    ELEMENTS_PICKED, ElementBox, ElementStyle, FRAMES_A_SECOND, KeyInput, KeyKind, MAX_HTML_CHARS,
    MAX_TEXT_CHARS, Modifiers, MouseInput, MouseKind, PageDialog, PickedElement, Viewport,
    follow_browsers, with_elements,
};
pub use shellenv::learn_shell_path;

pub use apps::{AppCsp, AppPolicy, AppShown, MAX_APP_MB, app_policy};
pub use changes::{ChangeKind, FileChange, FileDiff};
pub use chats::CHATS;
pub use checkpoints::{KEPT_TURNS, MAX_UNTRACKED_BYTES, SkippedFile, Turn, TurnChanges};
pub use cleanup::{Cleaned, Cleanup, CleanupItem, CleanupKind, NotCleaned};
pub use cloud::{CloudOrigin, CloudProvider, CloudTask, MAX_DELEGATED};
pub use commands::{CommandKind, SlashCommand};
pub use config::{Hooks, ProjectConfig};
pub use continuation::{CONTINUE_MESSAGE, follow_resets};
pub use costs::{Costs, DayCost, TaskCost};
pub use elsewhere::follow_others;
pub use environments::{Environment, EnvironmentChoice};
pub use error::{Error, Result};
pub use files::{FileEntry, FileListing};
pub use following::{
    FIRST_BACKOFF, MAX_BACKOFF, OPEN, PullChange, QUIET, QUIET_AFTER, RUNNING, follow_pulls,
    pull_wait,
};
pub use github_mentions::{
    FRESH_EVERY, GITHUB_LISTED, GitHubItem, GitHubItems, GitHubKind, GitHubState,
    MENTIONED_ON_GITHUB, with_github,
};
pub use handoff::MAX_HANDED;
pub use hooks::OutputSink;
pub use issues::{
    ISSUES_LISTED, Issue, IssueComment, IssueDetail, IssueFilter, IssueList, IssueState, IssueWho,
    MAX_ISSUES_LISTED,
};
pub use layouts::{
    Colour, Colours, CustomLayout, LayoutBase, LevelColours, SidebarLevel, SidebarLevels,
    TextSizes, layout_schema,
};
pub use marks::{MAX_MARK_LABEL_CHARS, Mark, MarkColour};
pub use mcp_own::{McpServerInput, McpServerView};
pub use mentions::{MENTIONED_SESSIONS, MENTIONED_SESSIONS_SAID, Mention, with_mentions};
pub use merging::{MergeMethod, MergeOutcome, MergeReport, MergeRules, MergeWhen};
pub use naming::TaskNaming;
pub use notices::Notice;
pub use open::open_link;
pub use overview::{ProjectOverview, TaskOverview};
pub use pasted::MAX_IMAGE_MB;
pub use paths::Paths;
pub use peers::{Delivery, MAX_SPAWNED, PeerTask, Reachable, serve_session_tools};
pub use prefix::{EXAMPLE_TASK, branch_example};
pub use publish::{PullDraft, Unpublished};
pub use pulls::{Check, CheckOutcome, Pull, PullState};
pub use put_away::{DoneWhy, follow_put_away};
pub use queue::QueuedMessage;
pub use returned::{MAX_PER_RESULT, MAX_RETURNED_MB, Returned};
pub use rewind::{RewindCheck, RewindDone};
pub use search::SearchHit;
pub use secrets::ApiKeyInfo;
use secrets::{MemorySecrets, SecretStore};
use sessions::Live;
pub use sessions::{
    EVENT_BUFFER, Session, SessionEvent, SessionOptions, SessionSettings, SessionStatus,
};
pub use settings::{
    BrowserDisplay, Layout, LimitWarnings, Palette, Settings, Style, Theme, ToolCalls, UpdateMode,
};
pub use setups::{SettingUp, SetupEnd, SetupLog, SetupProgress, SetupStep, SetupStop};
pub use skills::Skill;
pub use snapshots::Snapshot;
pub use stacks::StackState;
pub use starting::{FirstSession, SetupGoing, TaskStarted};
pub use statusline::StatusItem;
use store::Store;
pub use store::{Project, SetupStatus, Task};
pub use tasks::{BranchChoices, NewTaskOptions, RemoveOptions};
pub use templates::Template;
pub use termconfig::{Cursor, TerminalColour, TerminalColours, TerminalConfig, terminal_schema};
use terminal::Terminals;
pub use terminal::{TerminalOpened, TerminalOutput};
pub use toolserver::{SERVER_NAME, SHOW_BROWSER, SHOW_HTML, TOOL_SERVER_ARG, serve_tools};
pub use usage::{ProcessUsage, SessionUsage, Usage, WorktreeUsage};
pub use usage_copy::UsageAsk;
pub use views::VIEW_POLICY;
use views::ViewStore;

/// The entry point: one value per data directory, shared by everything a front end does.
#[derive(Debug)]
pub struct Core {
    paths: Paths,
    store: Arc<Mutex<Store>>,
    /// The agents sessions can run on. Behind a lock, since setting one up changes them.
    agents: RwLock<Agents>,
    /// Where programs such as agents and their installers are looked for.
    tool_dirs: Vec<PathBuf>,
    live: Arc<Live>,
    /// What gives new sessions their titles, if anything does.
    namer: Option<Arc<dyn Namer>>,
    /// Where API keys are kept.
    secrets: Box<dyn SecretStore>,
    /// The sessions' terminals.
    terminals: Arc<Terminals>,
    /// The Claude Code the sessions opened in a terminal, on a screen it has only there.
    claude_terminals: Arc<Terminals>,
    /// The changes to Swarmed's configuration that agents asked for and the user has not
    /// answered yet.
    proposals: configure::Proposals,
    /// HTML the agents wrote, waiting to be shown.
    views: ViewStore,
    /// The apps MCP servers attached to tool calls that are being shown (`apps.rs`).
    apps: apps::AppStore,
    /// The executable that serves Swarmed's own tools to agents, if a front end named one.
    tool_server: Option<PathBuf>,
    /// The lock that says this was the first Swarmed to arrive on its data, once it has it.
    house: Mutex<Option<std::fs::File>>,
    /// The names of tasks being created, by project id, from when each name is chosen until
    /// its task is written down, so that tasks created at once never pick the same name.
    names_claimed: Mutex<std::collections::HashSet<(i64, String)>>,
    /// The tasks' `setup` hooks this process runs now, so that one can be stopped on purpose.
    setups: Arc<setups::Setups>,
    /// What this core knows of other Swarmed processes on the same data.
    elsewhere: elsewhere::Elsewhere,
    /// How the pull requests of tasks are followed (`following.rs`).
    following: following::Following,
    /// What GitHub said each repository allows for merging (`merging.rs`).
    merging: merging::Merging,
    /// The tasks' browsers this process is connected to (`browser.rs`).
    browsers: Arc<browser::Browsers>,
}

/// The store is only ever locked for one short query, never across an `.await`. A panic while
/// it was held cannot leave SQLite half-written, so a poisoned lock is still usable.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Core {
    /// Opens the data directory, creating the database on first use. Sessions run on the
    /// installed Claude Code CLI, or on an ACP agent listed in `agents.toml`.
    pub fn open(paths: Paths) -> Result<Self> {
        let agents = Agents::load(&paths.agents_config(), &paths.opencode_config_dir)?;
        let mut core = Self::with_agents(paths, agents)?;
        core.set_namer(Arc::new(ClaudeNamer::default()));
        core.secrets = secrets::platform_store();
        Ok(core)
    }

    /// Like [`Core::open`], with `backend` standing in for Claude Code and no other agents.
    pub fn with_backend(paths: Paths, backend: Arc<dyn AgentBackend>) -> Result<Self> {
        Self::with_agents(paths, Agents::only_claude(backend))
    }

    fn with_agents(paths: Paths, agents: Agents) -> Result<Self> {
        let store = Store::open(&paths.db_file())?;
        Ok(Self {
            paths,
            store: Arc::new(Mutex::new(store)),
            agents: RwLock::new(agents),
            tool_dirs: agent_setup::default_tool_dirs(),
            live: Arc::default(),
            namer: None,
            // Nothing outside this process until `open` says where keys really live.
            secrets: Box::new(MemorySecrets::default()),
            terminals: Arc::default(),
            claude_terminals: Arc::default(),
            proposals: configure::Proposals::default(),
            views: ViewStore::default(),
            apps: apps::AppStore::default(),
            tool_server: None,
            house: Mutex::new(None),
            names_claimed: Mutex::default(),
            setups: Arc::default(),
            elsewhere: elsewhere::Elsewhere::default(),
            following: following::Following::default(),
            merging: merging::Merging::default(),
            browsers: Arc::default(),
        })
    }

    /// Gives every agent Swarmed's own tools, served by `executable` when it is started with
    /// [`TOOL_SERVER_ARG`]. A front end passes its own executable, which answers to that
    /// argument by calling [`serve_tools`].
    pub fn set_tool_server(&mut self, executable: PathBuf) {
        self.tool_server = Some(executable);
    }

    /// Looks for agents and their installers in `dirs` only, instead of on the `PATH`.
    pub fn set_tool_dirs(&mut self, dirs: Vec<PathBuf>) {
        self.tool_dirs = dirs;
    }

    /// The agents as they are now. A panic while the lock was held cannot leave them half
    /// changed, so a poisoned lock is still usable.
    fn agents_now(&self) -> RwLockReadGuard<'_, Agents> {
        self.agents.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn agents_mut(&mut self) -> &mut Agents {
        self.agents
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Lets `namer` give new sessions their titles, where the user's settings allow it.
    pub fn set_namer(&mut self, namer: Arc<dyn Namer>) {
        self.namer = Some(namer);
    }

    /// Adds an agent that runs on `backend`, replacing one with the same id.
    pub fn add_agent(&mut self, info: AgentInfo, backend: Arc<dyn AgentBackend>) {
        self.agents_mut().insert(info, backend);
    }

    /// The agents a session can be started on, Claude Code first. An agent that is configured
    /// but whose program is not on this machine is left out until it is installed.
    #[must_use]
    pub fn agents(&self) -> Vec<AgentInfo> {
        self.agents_now().available(&self.tool_dirs)
    }

    /// The locations this core reads and writes.
    #[must_use]
    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    fn store(&self) -> MutexGuard<'_, Store> {
        lock(&self.store)
    }

    /// Registers the git repository containing `path` and writes a starter config for it.
    /// Without `name`, the repository's directory name is used.
    pub async fn project_add(&self, path: &Path, name: Option<&str>) -> Result<Project> {
        let repo = git::repo_root(path).await?;
        let name = name.map_or_else(|| paths::name_from_path(&repo), str::to_string);
        paths::validate_project_name(&name)?;
        if self.store().project_by_name(&name)?.is_some() {
            return Err(Error::ProjectExists(name));
        }
        ProjectConfig::write_template_if_missing(&self.paths.project_config(&name))?;
        self.store().project_insert(&name, &repo)
    }

    /// All registered projects, by name.
    pub fn projects(&self) -> Result<Vec<Project>> {
        self.store().projects()
    }

    /// The project called `name`.
    pub fn project(&self, name: &str) -> Result<Project> {
        self.store()
            .project_by_name(name)?
            .ok_or_else(|| Error::ProjectNotFound(name.to_string()))
    }

    /// The project's local config, or the defaults if it has no config file.
    pub fn project_config(&self, project: &str) -> Result<ProjectConfig> {
        ProjectConfig::load(&self.paths.project_config(project))
    }
}

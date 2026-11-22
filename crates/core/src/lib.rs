//! The headless core of Swarmed.
//!
//! Every front end (the CLI, the desktop app) drives this crate and nothing else. It has no
//! UI dependency.

mod account_usage;
mod actions;
pub mod agent;
mod agent_models;
mod agent_setup;
mod agents;
mod analytics;
mod asking;
mod attach;
mod background;
mod changes;
mod cloud;
mod commands;
mod config;
mod configure;
mod costs;
mod effort;
mod environments;
mod error;
mod files;
mod fork;
mod git;
mod handoff;
pub mod hooks;
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
mod naming;
mod notes;
mod notices;
mod open;
mod order;
mod overview;
mod paths;
mod peers;
mod presence;
mod publish;
mod pulls;
mod queue;
mod reach;
mod search;
pub mod secrets;
mod sessions;
mod settings;
mod shellenv;
mod skills;
mod snapshots;
mod stacks;
mod statusline;
mod store;
mod tasks;
mod templates;
mod termconfig;
mod terminal;
mod toolserver;
mod usage;
mod vars;
mod views;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard};

pub use actions::Action;
use agent::{AgentBackend, ClaudeNamer, Namer};
pub use agent_setup::{AgentSetup, openrouter_models};
use agents::Agents;
pub use agents::{AgentInfo, CLAUDE};
pub use analytics::TurnStats;
pub use attach::{ATTACHED_FILES, with_files};
pub use background::{BackgroundOutput, BackgroundTask};
pub use shellenv::learn_shell_path;

pub use changes::{ChangeKind, FileChange, FileDiff};
pub use cloud::{CloudOrigin, CloudProvider, CloudTask, MAX_DELEGATED};
pub use commands::{CommandKind, SlashCommand};
pub use config::{Hooks, ProjectConfig};
pub use costs::{Costs, DayCost, TaskCost};
pub use environments::{Environment, EnvironmentChoice};
pub use error::{Error, Result};
pub use files::{FileEntry, FileListing};
pub use handoff::MAX_HANDED;
pub use hooks::OutputSink;
pub use layouts::{Colour, Colours, CustomLayout, LayoutBase, TextSizes, layout_schema};
pub use marks::{MAX_MARK_LABEL_CHARS, Mark, MarkColour};
pub use mcp_own::{McpServerInput, McpServerView};
pub use naming::TaskNaming;
pub use notices::Notice;
pub use open::open_link;
pub use overview::{ProjectOverview, TaskOverview};
pub use paths::Paths;
pub use peers::{Delivery, MAX_SPAWNED, serve_session_tools};
pub use publish::{PullDraft, Unpublished};
pub use pulls::{Check, CheckOutcome, Pull, PullState};
pub use queue::QueuedMessage;
pub use search::SearchHit;
pub use secrets::ApiKeyInfo;
use secrets::{MemorySecrets, SecretStore};
use sessions::Live;
pub use sessions::{
    EVENT_BUFFER, Session, SessionEvent, SessionOptions, SessionSettings, SessionStatus,
};
pub use settings::{Layout, Palette, Settings, Style, Theme, ToolCalls, UpdateMode};
pub use skills::Skill;
pub use snapshots::Snapshot;
pub use stacks::StackState;
pub use statusline::StatusItem;
use store::Store;
pub use store::{Project, SetupStatus, Task};
pub use tasks::{BranchChoices, NewTaskOptions, RemoveOptions};
pub use templates::Template;
pub use termconfig::{Cursor, TerminalColour, TerminalColours, TerminalConfig, terminal_schema};
use terminal::Terminals;
pub use terminal::{TerminalOpened, TerminalOutput};
pub use toolserver::{SERVER_NAME, SHOW_HTML, TOOL_SERVER_ARG, serve_tools};
pub use usage::{ProcessUsage, SessionUsage, Usage, WorktreeUsage};
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
    /// The executable that serves Swarmed's own tools to agents, if a front end named one.
    tool_server: Option<PathBuf>,
    /// The lock that says this was the first Swarmed to arrive on its data, once it has it.
    house: Mutex<Option<std::fs::File>>,
    /// The names of tasks being created, by project id, from when each name is chosen until
    /// its task is written down, so that tasks created at once never pick the same name.
    names_claimed: Mutex<std::collections::HashSet<(i64, String)>>,
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
            tool_server: None,
            house: Mutex::new(None),
            names_claimed: Mutex::default(),
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

//! The headless core of Swarmed.
//!
//! Every front end (the CLI, the desktop app) drives this crate and nothing else. It has no
//! UI dependency.

pub mod agent;
mod agents;
mod changes;
mod config;
mod error;
mod git;
pub mod hooks;
pub mod mcp;
mod naming;
mod notes;
mod open;
mod overview;
mod paths;
mod queue;
pub mod secrets;
mod sessions;
mod settings;
mod statusline;
mod store;
mod tasks;
mod terminal;
mod toolserver;
mod vars;
mod views;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use agent::{AgentBackend, ClaudeNamer, Namer};
use agents::Agents;
pub use agents::{AgentInfo, CLAUDE};

pub use changes::{ChangeKind, FileChange, FileDiff};
pub use config::{Hooks, ProjectConfig};
pub use error::{Error, Result};
pub use hooks::OutputSink;
pub use naming::TaskNaming;
pub use overview::{ProjectOverview, TaskOverview};
pub use paths::Paths;
pub use queue::QueuedMessage;
pub use secrets::ApiKeyInfo;
use secrets::{MemorySecrets, SecretStore};
use sessions::Live;
pub use sessions::{
    EVENT_BUFFER, Session, SessionEvent, SessionOptions, SessionSettings, SessionStatus,
};
pub use settings::{Settings, Theme};
pub use statusline::StatusItem;
use store::Store;
pub use store::{Project, SetupStatus, Task};
pub use tasks::{BranchChoices, NewTaskOptions, RemoveOptions};
pub use terminal::TerminalOutput;
use terminal::Terminals;
pub use toolserver::{SERVER_NAME, SHOW_HTML, TOOL_SERVER_ARG, serve_tools};
pub use views::VIEW_POLICY;
use views::ViewStore;

/// The entry point: one value per data directory, shared by everything a front end does.
#[derive(Debug)]
pub struct Core {
    paths: Paths,
    store: Arc<Mutex<Store>>,
    agents: Agents,
    live: Arc<Live>,
    /// What gives new sessions their titles, if anything does.
    namer: Option<Arc<dyn Namer>>,
    /// Where API keys are kept.
    secrets: Box<dyn SecretStore>,
    /// The sessions' terminals.
    terminals: Arc<Terminals>,
    /// HTML the agents wrote, waiting to be shown.
    views: ViewStore,
    /// The executable that serves Swarmed's own tools to agents, if a front end named one.
    tool_server: Option<PathBuf>,
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
        let agents = Agents::load(&paths.agents_config())?;
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
            agents,
            live: Arc::default(),
            namer: None,
            // Nothing outside this process until `open` says where keys really live.
            secrets: Box::new(MemorySecrets::default()),
            terminals: Arc::default(),
            views: ViewStore::default(),
            tool_server: None,
        })
    }

    /// Gives every agent Swarmed's own tools, served by `executable` when it is started with
    /// [`TOOL_SERVER_ARG`]. A front end passes its own executable, which answers to that
    /// argument by calling [`serve_tools`].
    pub fn set_tool_server(&mut self, executable: PathBuf) {
        self.tool_server = Some(executable);
    }

    /// Lets `namer` give new sessions their titles, where the user's settings allow it.
    pub fn set_namer(&mut self, namer: Arc<dyn Namer>) {
        self.namer = Some(namer);
    }

    /// Adds an agent that runs on `backend`, replacing one with the same id.
    pub fn add_agent(&mut self, info: AgentInfo, backend: Arc<dyn AgentBackend>) {
        self.agents.insert(info, backend);
    }

    /// The agents a session can run on, Claude Code first.
    #[must_use]
    pub fn agents(&self) -> Vec<AgentInfo> {
        self.agents.infos()
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
        paths::validate_name(&name)?;
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

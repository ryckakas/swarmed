//! The headless core of Swarmed.
//!
//! Every front end (the CLI, the desktop app) drives this crate and nothing else. It has no
//! UI dependency.

pub mod agent;
mod config;
mod error;
mod git;
pub mod hooks;
mod paths;
mod sessions;
mod store;
mod tasks;
mod vars;

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use agent::{AgentBackend, ClaudeBackend};

pub use config::{Hooks, ProjectConfig};
pub use error::{Error, Result};
pub use hooks::OutputSink;
pub use paths::Paths;
use sessions::Live;
pub use sessions::{EVENT_BUFFER, Session, SessionEvent, SessionOptions, SessionStatus};
use store::Store;
pub use store::{Project, SetupStatus, Task};
pub use tasks::{NewTaskOptions, RemoveOptions};

/// The entry point: one value per data directory, shared by everything a front end does.
#[derive(Debug)]
pub struct Core {
    paths: Paths,
    store: Arc<Mutex<Store>>,
    backend: Arc<dyn AgentBackend>,
    live: Arc<Live>,
}

/// The store is only ever locked for one short query, never across an `.await`. A panic while
/// it was held cannot leave SQLite half-written, so a poisoned lock is still usable.
fn lock(store: &Mutex<Store>) -> MutexGuard<'_, Store> {
    store.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Core {
    /// Opens the data directory, creating the database on first use. Sessions run on the
    /// installed Claude Code CLI.
    pub fn open(paths: Paths) -> Result<Self> {
        Self::with_backend(paths, Arc::new(ClaudeBackend::default()))
    }

    /// Like [`Core::open`], with sessions running on `backend`.
    pub fn with_backend(paths: Paths, backend: Arc<dyn AgentBackend>) -> Result<Self> {
        let store = Store::open(&paths.db_file())?;
        Ok(Self {
            paths,
            store: Arc::new(Mutex::new(store)),
            backend,
            live: Arc::default(),
        })
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

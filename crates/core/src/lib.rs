//! The headless core of Swarmed.
//!
//! Every front end (the CLI, the desktop app) drives this crate and nothing else. It has no
//! UI dependency.

mod config;
mod error;
mod git;
pub mod hooks;
mod paths;
mod store;
mod tasks;
mod vars;

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

pub use config::{Hooks, ProjectConfig};
pub use error::{Error, Result};
pub use hooks::OutputSink;
pub use paths::Paths;
use store::Store;
pub use store::{Project, SetupStatus, Task};
pub use tasks::{NewTaskOptions, RemoveOptions};

/// The entry point: one value per data directory, shared by everything a front end does.
#[derive(Debug)]
pub struct Core {
    paths: Paths,
    store: Mutex<Store>,
}

impl Core {
    /// Opens the data directory, creating the database on first use.
    pub fn open(paths: Paths) -> Result<Self> {
        let store = Store::open(&paths.db_file())?;
        Ok(Self {
            paths,
            store: Mutex::new(store),
        })
    }

    /// The locations this core reads and writes.
    #[must_use]
    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// The store is only ever locked for one short query, never across an `.await`. A panic
    /// while it was held cannot leave SQLite half-written, so a poisoned lock is still usable.
    fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
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

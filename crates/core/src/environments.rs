//! Named environments of a project: different ways of setting a task up on this machine.
//!
//! An environment is a `setup` and a `teardown` command, environment variables and the address
//! of what it runs. Each is laid over the project's own configuration, so one says only what
//! it does differently. A task is created in one of them, or in none, and keeps it.
//!
//! They live in a file of their own beside the project's configuration, written from a front
//! end. Their variables may hold credentials: the file is readable by the user alone, and the
//! values are never logged.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Core;
use crate::config::ProjectConfig;
use crate::error::{Error, Result};
use crate::paths;

/// One environment of a project. Whatever is left empty is as the project has it.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Environment {
    /// Runs once after a task's worktree is created, in place of the project's `setup` hook.
    pub setup: String,
    /// Runs before a task's worktree is removed, in place of the project's `teardown` hook.
    pub teardown: String,
    /// Where a task's running stack is reached in a browser, in place of the project's.
    pub open_url: String,
    /// Variables for the hooks and for the agents working in the task, on top of the
    /// project's. Values may use the same `{{variables}}` as the project's.
    pub env: BTreeMap<String, String>,
}

impl std::fmt::Debug for Environment {
    /// Prints the variables' names and never their values.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Environment")
            .field("setup", &self.setup)
            .field("teardown", &self.teardown)
            .field("open_url", &self.open_url)
            .field("env", &self.env.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Environment {
    /// `config` as a task in this environment sees it.
    fn over(&self, mut config: ProjectConfig) -> ProjectConfig {
        replace(&self.setup, &mut config.hooks.setup);
        replace(&self.teardown, &mut config.hooks.teardown);
        replace(&self.open_url, &mut config.open_url);
        config.env.extend(self.env.clone());
        config
    }
}

/// Puts `own` in `target`'s place, unless it is blank.
fn replace(own: &str, target: &mut String) {
    if !own.trim().is_empty() {
        *target = own.to_string();
    }
}

type Environments = BTreeMap<String, Environment>;

fn read(path: &Path) -> Result<Environments> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Environments::new()),
        Err(error) => return Err(error.into()),
    };
    toml::from_str(&text).map_err(|error| invalid(path, &error))
}

fn invalid(path: &Path, error: &dyn std::fmt::Display) -> Error {
    Error::Config {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

/// Writes the file for the user alone: a variable's value may be a password.
fn write(path: &Path, environments: &Environments) -> Result<()> {
    let text = toml::to_string(environments).map_err(|error| invalid(path, &error))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let header = "# The environments of this project. Written from Swarmed; editing by hand is \
                  fine.\n\n";
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true).mode(0o600);
    let mut file = options.open(path)?;
    file.write_all(format!("{header}{text}").as_bytes())?;
    Ok(())
}

impl Core {
    /// The environments of `project`, by name.
    pub fn environments(&self, project: &str) -> Result<BTreeMap<String, Environment>> {
        let project = self.project(project)?;
        read(&self.paths.project_environments(&project.name))
    }

    /// Adds an environment to `project`, or replaces the one called `name`. Tasks already in
    /// it get the change from their next hook and their next agent start.
    pub fn environment_save(
        &self,
        project: &str,
        name: &str,
        environment: &Environment,
    ) -> Result<()> {
        paths::validate_name(name)?;
        let invalid_variable = |variable: &&String| !is_variable_name(variable);
        if let Some(variable) = environment.env.keys().find(invalid_variable) {
            return Err(Error::InvalidVariable(variable.clone()));
        }
        let project = self.project(project)?;
        let path = self.paths.project_environments(&project.name);
        let mut environments = read(&path)?;
        environments.insert(name.to_string(), environment.clone());
        write(&path, &environments)
    }

    /// Removes an environment. Tasks that were in it go on as the project itself is set up.
    pub fn environment_remove(&self, project: &str, name: &str) -> Result<()> {
        let project = self.project(project)?;
        let path = self.paths.project_environments(&project.name);
        let mut environments = read(&path)?;
        if environments.remove(name).is_some() {
            write(&path, &environments)?;
        }
        Ok(())
    }

    /// The project's configuration as a task in `environment` sees it. An environment that is
    /// gone, or none, leaves the project's own.
    pub(crate) fn config_in(
        &self,
        project: &str,
        environment: Option<&str>,
    ) -> Result<ProjectConfig> {
        let config = self.project_config(project)?;
        let Some(name) = environment else {
            return Ok(config);
        };
        let mut environments = read(&self.paths.project_environments(project))?;
        Ok(match environments.remove(name) {
            Some(environment) => environment.over(config),
            None => config,
        })
    }

    /// Fails unless `project` has an environment called `name`.
    pub(crate) fn require_environment(&self, project: &str, name: &str) -> Result<()> {
        let environments = read(&self.paths.project_environments(project))?;
        if environments.contains_key(name) {
            return Ok(());
        }
        Err(Error::EnvironmentNotFound {
            project: project.to_string(),
            environment: name.to_string(),
        })
    }
}

/// Whether `name` can be the name of an environment variable.
fn is_variable_name(name: &str) -> bool {
    let start = |c: char| c.is_ascii_alphabetic() || c == '_';
    let rest = |c: char| c.is_ascii_alphanumeric() || c == '_';
    name.starts_with(start) && name.chars().all(rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_environment_says_only_what_it_does_differently() {
        let mut config = ProjectConfig::default();
        config.hooks.setup = "npm ci".to_string();
        config.hooks.teardown = "rm -rf node_modules".to_string();
        config.env.insert("A".into(), "project".into());
        config.env.insert("B".into(), "project".into());
        let staging = Environment {
            setup: "npm ci && npm run seed:staging".to_string(),
            open_url: "http://localhost:{{port.web}}".to_string(),
            env: BTreeMap::from([("B".to_string(), "staging".to_string())]),
            ..Environment::default()
        };
        let seen = staging.over(config);
        assert_eq!(seen.hooks.setup, "npm ci && npm run seed:staging");
        assert_eq!(seen.hooks.teardown, "rm -rf node_modules");
        assert_eq!(seen.open_url, "http://localhost:{{port.web}}");
        assert_eq!(seen.env["A"], "project");
        assert_eq!(seen.env["B"], "staging");
    }

    #[test]
    fn variables_are_named_as_a_shell_would_take_them_and_their_values_stay_out_of_debug() {
        assert!(is_variable_name("DATABASE_URL") && is_variable_name("_x1"));
        assert!(!is_variable_name("1X") && !is_variable_name("A-B") && !is_variable_name(""));
        let secret = Environment {
            env: BTreeMap::from([("TOKEN".to_string(), "sk-secret".to_string())]),
            ..Environment::default()
        };
        let printed = format!("{secret:?}");
        assert!(printed.contains("TOKEN") && !printed.contains("sk-secret"));
    }
}

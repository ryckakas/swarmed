//! Named environments of a project: different ways of setting a task up on this machine.
//!
//! An environment is a `setup` command, a `start` command for what is to keep running, `stop`
//! and `teardown` commands, environment variables, the address of what it runs and how many
//! stacks may run at once. Each is laid over the project's own configuration, so one says only
//! what it does differently. A task is created in one of them, or in none, and keeps it.
//!
//! They live in a file of their own beside the project's configuration, written from a front
//! end. Their variables may hold credentials: the file is readable by the user alone, and the
//! values are never logged.
//!
//! One of them can be the project's default, which a new task is set up in when none is
//! chosen. That is kept in the database, not in either file: a Swarmed sharing the data
//! directory that does not know the key would refuse the file, as 0.3.0 does, or name the key
//! as a problem every time it starts. A column it does not know, it never sees.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Core;
use crate::config::ProjectConfig;
use crate::error::{Error, Result};
use crate::lenient::{self, Problems};
use crate::paths;

/// One environment of a project. Whatever is left empty is as the project has it. A key it
/// does not know is read past and named (`lenient`).
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Environment {
    /// Runs once after a task's worktree is created, in place of the project's `setup` hook.
    pub setup: String,
    /// Started once `setup` is done and left running, in place of the project's `start` hook:
    /// a dev server, or whatever else the task needs up while it is worked on.
    pub start: String,
    /// Runs when the stack is stopped, before what `start` left running is ended, in place of
    /// the project's `stop` hook.
    pub stop: String,
    /// Runs before a task's worktree is removed, in place of the project's `teardown` hook.
    pub teardown: String,
    /// Where a task's running stack is reached in a browser, in place of the project's.
    pub open_url: String,
    /// Named base ports, on top of the project's: a task gets `base + slot * port_step` as
    /// `{{port.<name>}}`, so that stacks running side by side do not meet.
    pub ports: BTreeMap<String, u16>,
    /// Variables for the hooks and for the agents working in the task, on top of the
    /// project's. Values may use the same `{{variables}}` as the project's.
    pub env: BTreeMap<String, String>,
    /// How many of the project's stacks may run for one of this environment's tasks to start
    /// its own, in place of the project's `max_stacks`. `None` leaves the project's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_stacks: Option<u32>,
}

impl std::fmt::Debug for Environment {
    /// Prints the variables' names and never their values.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Environment")
            .field("setup", &self.setup)
            .field("start", &self.start)
            .field("stop", &self.stop)
            .field("teardown", &self.teardown)
            .field("open_url", &self.open_url)
            .field("ports", &self.ports)
            .field("env", &self.env.keys().collect::<Vec<_>>())
            .field("max_stacks", &self.max_stacks)
            .finish()
    }
}

impl Environment {
    /// `config` as a task in this environment sees it.
    fn over(&self, mut config: ProjectConfig) -> ProjectConfig {
        replace(&self.setup, &mut config.hooks.setup);
        replace(&self.start, &mut config.hooks.start);
        replace(&self.stop, &mut config.hooks.stop);
        replace(&self.teardown, &mut config.hooks.teardown);
        replace(&self.open_url, &mut config.open_url);
        config.ports.extend(self.ports.clone());
        config.env.extend(self.env.clone());
        if let Some(limit) = self.max_stacks {
            config.max_stacks = limit;
        }
        config
    }
}

/// Which of a project's environments a new task is set up in.
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentChoice {
    /// The project's default environment, or its own setup when it has none.
    #[default]
    Default,
    /// The project's own setup, whatever its default.
    Own,
    /// The environment of this name, which the project must have.
    Named(String),
}

/// Puts `own` in `target`'s place, unless it is blank.
fn replace(own: &str, target: &mut String) {
    if !own.trim().is_empty() {
        *target = own.to_string();
    }
}

type Environments = BTreeMap<String, Environment>;

/// The environments that can be read. One that cannot is left out (`problems_in` says why),
/// so tasks in the others go on as before. A file that is not TOML is an error: there is no
/// telling which environment was which.
fn read(path: &Path) -> Result<Environments> {
    Ok(read_with_problems(path)?.0)
}

/// The environments that can be read, with what could not, said for the user.
fn read_with_problems(path: &Path) -> Result<(Environments, Problems)> {
    let Some(text) = lenient::read_if_there(path)? else {
        return Ok((Environments::new(), Vec::new()));
    };
    let table = text.parse::<toml::Table>().map_err(|error| {
        let meanwhile = "Tasks in its environments cannot be set up or started until it is fixed.";
        invalid(path, &lenient::not_toml(&text, &error, meanwhile))
    })?;
    Ok(lenient::entries::<Environment>(table))
}

/// What of the environments file at `path` cannot be understood, said for the user.
pub(crate) fn problems_in(path: &Path) -> Result<Problems> {
    Ok(read_with_problems(path)?.1)
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
        let invalid_port = |port: &&String| !is_port_name(port);
        if let Some(port) = environment.ports.keys().find(invalid_port) {
            return Err(Error::InvalidPortName(port.clone()));
        }
        if environment.max_stacks == Some(0) {
            return Err(Error::NoRoomForAStack);
        }
        let project = self.project(project)?;
        let path = self.paths.project_environments(&project.name);
        let mut environments = read(&path)?;
        environments.insert(name.to_string(), environment.clone());
        write(&path, &environments)
    }

    /// Removes an environment. Refused while a task still uses it: its stack, `stop` and
    /// `teardown` come from the environment, and without them the task could no longer be
    /// stopped and removed as it was set up.
    pub fn environment_remove(&self, project: &str, name: &str) -> Result<()> {
        let project = self.project(project)?;
        let users: Vec<String> = self
            .store()
            .tasks(project.id, false)?
            .into_iter()
            .filter(|task| task.environment.as_deref() == Some(name))
            .map(|task| task.name)
            .collect();
        if !users.is_empty() {
            return Err(Error::EnvironmentInUse {
                environment: name.to_string(),
                tasks: users.join(", "),
            });
        }
        let path = self.paths.project_environments(&project.name);
        let mut environments = read(&path)?;
        if environments.remove(name).is_some() {
            write(&path, &environments)?;
        }
        let store = self.store();
        if store.project_default_environment(project.id)?.as_deref() == Some(name) {
            store.project_set_default_environment(project.id, None)?;
        }
        Ok(())
    }

    /// The environment new tasks of `project` are set up in when none is chosen, if it has
    /// one. A default that is no longer among its environments, taken out of the file by hand
    /// or by an older Swarmed, is none.
    pub fn environment_default(&self, project: &str) -> Result<Option<String>> {
        let project = self.project(project)?;
        let Some(name) = self.store().project_default_environment(project.id)? else {
            return Ok(None);
        };
        let environments = read(&self.paths.project_environments(&project.name))?;
        Ok(environments.contains_key(&name).then_some(name))
    }

    /// Makes `name` the environment new tasks of `project` are set up in when none is chosen;
    /// `None` leaves them to the project's own setup. Tasks already made keep theirs.
    pub fn environment_set_default(&self, project: &str, name: Option<&str>) -> Result<()> {
        let project = self.project(project)?;
        if let Some(name) = name {
            self.require_environment(&project.name, name)?;
        }
        self.store()
            .project_set_default_environment(project.id, name)
    }

    /// The environment `choice` comes to for a new task of `project`: `None` for the project's
    /// own setup. A named one the project does not have is refused.
    pub(crate) fn environment_chosen(
        &self,
        project: &str,
        choice: &EnvironmentChoice,
    ) -> Result<Option<String>> {
        match choice {
            EnvironmentChoice::Default => self.environment_default(project),
            EnvironmentChoice::Own => Ok(None),
            EnvironmentChoice::Named(name) => {
                self.require_environment(project, name)?;
                Ok(Some(name.clone()))
            }
        }
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

/// Whether `name` can be the name of a port: it is written as `{{port.<name>}}` and exported
/// as `SWARMED_PORT_<NAME>`.
fn is_port_name(name: &str) -> bool {
    let plain = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_';
    name.starts_with(|c: char| c.is_ascii_lowercase()) && name.chars().all(plain)
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
        assert_eq!(seen.max_stacks, ProjectConfig::default().max_stacks);
    }

    #[test]
    fn an_environments_stack_limit_takes_the_place_of_the_projects() {
        let config = ProjectConfig {
            max_stacks: 1,
            ..ProjectConfig::default()
        };
        let roomy = Environment {
            max_stacks: Some(3),
            ..Environment::default()
        };
        assert_eq!(roomy.over(config).max_stacks, 3);
    }

    #[test]
    fn an_environment_without_a_stack_limit_is_written_without_one() {
        let plain = toml::to_string(&Environment::default()).expect("writes");
        assert!(!plain.contains("max_stacks"), "{plain}");
        let roomy = Environment {
            max_stacks: Some(4),
            ..Environment::default()
        };
        let written = toml::to_string(&roomy).expect("writes");
        assert_eq!(toml::from_str::<Environment>(&written).ok(), Some(roomy));
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

    #[test]
    fn an_environments_ports_add_to_the_projects_and_must_be_usable_names() {
        let mut config = ProjectConfig::default();
        config.ports.insert("db".into(), 5432);
        let local = Environment {
            ports: BTreeMap::from([("web".to_string(), 3000), ("db".to_string(), 6000)]),
            ..Environment::default()
        };
        let seen = local.over(config);
        assert_eq!(
            seen.ports,
            BTreeMap::from([("db".to_string(), 6000), ("web".to_string(), 3000)])
        );
        assert!(is_port_name("web_2"));
        assert!(
            !is_port_name("Web")
                && !is_port_name("2nd")
                && !is_port_name("a-b")
                && !is_port_name("")
        );
    }
}

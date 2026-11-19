use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// A project's local configuration, one TOML file per project in the data directory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectConfig {
    /// Branch name for a new task. Variables: `user`, `task`, `project`.
    pub branch_pattern: String,
    /// Ref new tasks branch from. Empty means the remote's default branch.
    pub base_branch: String,
    /// How many stacks of this project may run at once.
    pub max_stacks: u32,
    /// A task's port is `base + slot * port_step`; slot 0 is left to the main checkout.
    pub port_step: u16,
    /// Named base ports, available to hooks as `{{port.<name>}}`.
    pub ports: BTreeMap<String, u16>,
    /// Where a task's running stack is reached in a browser, such as
    /// `http://localhost:{{port.web}}`. Empty for a project with nothing to look at.
    pub open_url: String,
    /// The commands that build, run and destroy a task's stack.
    pub hooks: Hooks,
    /// Extra environment for hooks. Values may use variables.
    pub env: BTreeMap<String, String>,
}

/// A project's hook commands. Each runs with `sh -c` in the task's worktree; an empty one is
/// skipped.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Hooks {
    /// Runs once after the worktree is created: install dependencies, create and seed the database.
    pub setup: String,
    /// Starts the task's dev stack.
    pub start: String,
    /// Stops the task's dev stack.
    pub stop: String,
    /// Runs before the worktree is removed: destroy containers, volumes and anything else `setup` made.
    pub teardown: String,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            branch_pattern: "{{user}}/{{task}}".to_string(),
            base_branch: String::new(),
            max_stacks: 2,
            port_step: 100,
            ports: BTreeMap::new(),
            open_url: String::new(),
            hooks: Hooks::default(),
            env: BTreeMap::new(),
        }
    }
}

const TEMPLATE: &str = r#"# Swarmed project config. This file is local to this machine.
#
# Variables usable in branch_pattern, hooks and env values:
#   {{project}} {{task}} {{user}} {{branch}} {{slot}} {{worktree}} {{repo}} {{port.<name>}}
# open_url may use them too.
# Hooks run with `sh -c` in the task's worktree. The same values are also exported as
# SWARMED_PROJECT, SWARMED_TASK, SWARMED_SLOT, SWARMED_WORKTREE, SWARMED_REPO, SWARMED_BRANCH
# and SWARMED_PORT_<NAME>.

branch_pattern = "{{user}}/{{task}}"
# base_branch = "origin/main"   # default: the remote's default branch
max_stacks = 2
port_step = 100                 # task port = base + slot * port_step
# open_url = "http://localhost:{{port.web}}"   # what "Open in browser" opens for a task

[ports]
# web = 3000
# db = 5432

[hooks]
# setup = "cp {{repo}}/.env .env && npm ci"
# start = "docker compose -p {{project}}-{{task}} up -d"
# stop = "docker compose -p {{project}}-{{task}} stop"
# teardown = "docker compose -p {{project}}-{{task}} down -v"

[env]
# COMPOSE_PROJECT_NAME = "{{project}}-{{task}}"
# WEB_PORT = "{{port.web}}"
"#;

impl ProjectConfig {
    /// Loads the config, or returns the defaults if the file does not exist.
    pub(crate) fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        let config: Self = toml::from_str(&text).map_err(|e| Error::Config {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        if config.port_step == 0 {
            return Err(Error::Config {
                path: path.to_path_buf(),
                message: "port_step must be at least 1".to_string(),
            });
        }
        Ok(config)
    }

    /// Writes the commented starter config unless a file is already there.
    pub(crate) fn write_template_if_missing(path: &Path) -> Result<()> {
        if path.exists() {
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, TEMPLATE)?;
        Ok(())
    }

    /// The port a task in `slot` gets for a port whose base is `base`, or `None` on overflow.
    #[must_use]
    pub fn port_for(&self, base: u16, slot: u32) -> Option<u16> {
        let offset = slot.checked_mul(u32::from(self.port_step))?;
        u16::try_from(u32::from(base).checked_add(offset)?).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_parses_to_defaults() {
        assert_eq!(
            toml::from_str::<ProjectConfig>(TEMPLATE).ok(),
            Some(ProjectConfig::default())
        );
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(toml::from_str::<ProjectConfig>("branch_patern = \"x\"").is_err());
    }

    #[test]
    fn ports_are_offset_by_slot() {
        let config = ProjectConfig::default();
        assert_eq!(config.port_for(3000, 2), Some(3200));
        assert_eq!(config.port_for(65000, 9), None);
    }
}

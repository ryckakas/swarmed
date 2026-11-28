use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::lenient::{self, Problems};

/// A project's local configuration, one TOML file per project in the data directory. A key it
/// does not know is read past and named (`ProjectConfig::read`), never refused.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ProjectConfig {
    /// Branch name for a new task. Variables: `prefix` (the branch prefix of the settings, or
    /// the user's login when it is empty), `user`, `task`, `project`.
    pub branch_pattern: String,
    /// Ref new tasks branch from. Empty means the remote's default branch.
    pub base_branch: String,
    /// How many stacks of this project may run at once. Starting one more is refused. An
    /// environment may set its own, for the stacks of its tasks.
    pub max_stacks: u32,
    /// How many of this project's tasks may run their `setup` at once in one Swarmed; those
    /// over it wait for a place, and say so. 0, the default, is no limit. An environment may
    /// set its own, for the setups of its tasks.
    pub max_setups: u32,
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
    /// The tracked files the dependencies are pinned in, such as `pnpm-lock.yaml`: once one
    /// differs from what the snapshot was taken with, the next task started from the base
    /// takes its place (`snapshots.rs`). Empty, the first task set up well stays the snapshot.
    /// An environment may name its own.
    pub snapshot_keys: Vec<String>,
}

/// A project's hook commands. Each runs with `sh -c` in the task's worktree; an empty one is
/// skipped. A `{{variable}}` in one is filled in as one word of the command, whatever its
/// value holds.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
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

/// The branch pattern of a repository that sets none of its own.
pub(crate) const DEFAULT_BRANCH_PATTERN: &str = "{{prefix}}/{{task}}";

/// The default branch pattern before there was a branch prefix, which the template wrote into
/// every new config. Found in one, it was Swarmed's choice and not the user's.
const EARLIER_DEFAULT_PATTERN: &str = "{{user}}/{{task}}";

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            branch_pattern: DEFAULT_BRANCH_PATTERN.to_string(),
            base_branch: String::new(),
            max_stacks: 5,
            max_setups: 0,
            port_step: 100,
            ports: BTreeMap::new(),
            open_url: String::new(),
            hooks: Hooks::default(),
            env: BTreeMap::new(),
            snapshot_keys: Vec::new(),
        }
    }
}

// The branch pattern is only a comment, so that each Swarmed goes by its own default: an older
// one sharing the data directory knows no `{{prefix}}`, and could not make a task with it.
const TEMPLATE: &str = r#"# Swarmed project config. This file is local to this machine.
#
# Variables usable in branch_pattern, hooks and env values:
#   {{project}} {{task}} {{user}} {{branch}} {{slot}} {{worktree}} {{repo}} {{port.<name>}}
# open_url may use them too. branch_pattern may also use {{prefix}}: the branch prefix in
# Swarmed's settings, or your login ({{user}}) when that is empty.
# Hooks run with `sh -c` in the task's worktree. The same values are also exported as
# SWARMED_PROJECT, SWARMED_TASK, SWARMED_SLOT, SWARMED_WORKTREE, SWARMED_REPO, SWARMED_BRANCH
# and SWARMED_PORT_<NAME>.
# In a hook a value is always one word, whatever is in it: a path with a space needs no
# quotes around it, and a branch's name is never read as a command.

# branch_pattern = "{{prefix}}/{{task}}"   # default: the branch prefix, a slash, the task
# base_branch = "origin/main"   # default: the remote's default branch
max_stacks = 5
# max_setups = 2                # tasks set up at once; the others wait. Default: no limit
port_step = 100                 # task port = base + slot * port_step
# open_url = "http://localhost:{{port.web}}"   # what "Open in browser" opens for a task;
#                                              # without it, the local addresses `start` prints
# snapshot_keys = ["pnpm-lock.yaml"]   # files dependencies are pinned in: once they change on
#                                      # the base branch, a new task keeps a newer snapshot

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
    /// Loads the config, or returns the defaults if the file does not exist. What of it cannot be
    /// understood is at its default (`read` says what).
    pub(crate) fn load(path: &Path) -> Result<Self> {
        Ok(Self::read(path)?.0)
    }

    /// Reads the config key by key, with what could not be understood, said for the user: a key
    /// it does not know is left out, and a value it cannot take is at its default. Only a file
    /// that is not TOML is an error. There is no telling what its hooks and ports were, and
    /// tasks set up or removed without them could leave behind what `teardown` would have
    /// ended.
    pub(crate) fn read(path: &Path) -> Result<(Self, Problems)> {
        let Some(text) = lenient::read_if_there(path)? else {
            return Ok((Self::default(), Vec::new()));
        };
        let table = text.parse::<toml::Table>().map_err(|error| Error::Config {
            path: path.to_path_buf(),
            message: lenient::not_toml(
                &text,
                &error,
                "Its tasks cannot be set up, started or removed with their hooks until it is fixed.",
            ),
        })?;
        let (mut config, mut problems) = lenient::fields::<Self>(table);
        // Swarmed's own earlier default, not a pattern the user chose: the branch prefix
        // applies to it as to a config that sets none.
        if config.branch_pattern == EARLIER_DEFAULT_PATTERN {
            config.branch_pattern = DEFAULT_BRANCH_PATTERN.to_string();
        }
        if config.port_step == 0 {
            config.port_step = Self::default().port_step;
            problems.push("`port_step` must be at least 1, so it is at its default.".to_string());
        }
        Ok((config, problems))
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
    fn what_cannot_be_understood_is_named_and_costs_only_itself() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("demo.toml");
        let text = "branch_patern = \"x\"\nport_step = 0\nmax_stacks = 3\n\
                    [hooks]\nsetup = \"make\"\nwarmup = \"later\"\n";
        std::fs::write(&path, text).expect("writes");
        let (config, problems) = ProjectConfig::read(&path).expect("reads");
        assert_eq!((config.max_stacks, config.port_step), (3, 100));
        assert_eq!(config.hooks.setup, "make");
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("`branch_patern`"))
        );
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("`hooks.warmup`"))
        );
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("`port_step`"))
        );
        // Not TOML at all, its hooks are unknown: that stays an error.
        std::fs::write(&path, "[hooks\n").expect("writes");
        assert!(matches!(
            ProjectConfig::read(&path),
            Err(Error::Config { .. })
        ));
    }

    #[test]
    fn the_earlier_default_pattern_is_read_as_the_default_and_ones_own_is_kept() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("demo.toml");
        let pattern = |text: &str| {
            std::fs::write(&path, text).expect("writes");
            ProjectConfig::load(&path).expect("reads").branch_pattern
        };
        // What the template wrote before there was a prefix.
        assert_eq!(
            pattern("branch_pattern = \"{{user}}/{{task}}\"\n"),
            DEFAULT_BRANCH_PATTERN
        );
        assert_eq!(pattern("max_stacks = 3\n"), DEFAULT_BRANCH_PATTERN);
        for own in [
            "{{user}}-{{task}}",
            "swarm/{{task}}",
            "{{prefix}}/{{project}}/{{task}}",
        ] {
            assert_eq!(pattern(&format!("branch_pattern = \"{own}\"\n")), own);
        }
    }

    #[test]
    fn ports_are_offset_by_slot() {
        let config = ProjectConfig::default();
        assert_eq!(config.port_for(3000, 2), Some(3200));
        assert_eq!(config.port_for(65000, 9), None);
    }
}

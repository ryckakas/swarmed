use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// What the name of every socket of Swarmed's starts with.
pub(crate) const SOCKET_PREFIX: &str = "swarmed-";

/// Where Swarmed keeps its own files. Nothing is ever written into a project's repo.
#[derive(Debug, Clone)]
pub struct Paths {
    /// Database and per-project config files.
    pub data_dir: PathBuf,
    /// Parent of all task worktrees: `<worktrees_root>/<project>/<task>`.
    pub worktrees_root: PathBuf,
    /// Claude Code's own config file, which is only ever read: it lists the user's MCP servers,
    /// and says when Claude Code last fetched the account's usage.
    pub claude_config: PathBuf,
    /// Codex's own config file, which is only ever read, to show the MCP servers it lists.
    pub codex_config: PathBuf,
    /// OpenCode's own configuration directory, which is only ever read, for the same reason
    /// and for the model it names as OpenCode's default, if any.
    pub opencode_config_dir: PathBuf,
}

impl Paths {
    /// The default locations, overridable with `SWARMED_DATA_DIR`, `SWARMED_WORKTREES_DIR`,
    /// `SWARMED_CLAUDE_CONFIG`, `SWARMED_CODEX_CONFIG` and `SWARMED_OPENCODE_CONFIG_DIR`.
    pub fn from_env() -> Result<Self> {
        let home = std::env::home_dir().ok_or(Error::NoHome)?;
        let data_dir = std::env::var_os("SWARMED_DATA_DIR")
            .map_or_else(|| default_data_dir(&home), PathBuf::from);
        let worktrees_root = std::env::var_os("SWARMED_WORKTREES_DIR")
            .map_or_else(|| home.join("swarmed"), PathBuf::from);
        let claude_config = std::env::var_os("SWARMED_CLAUDE_CONFIG")
            .map_or_else(|| home.join(".claude.json"), PathBuf::from);
        let codex_config = std::env::var_os("SWARMED_CODEX_CONFIG")
            .map_or_else(|| home.join(".codex/config.toml"), PathBuf::from);
        let opencode_config_dir = std::env::var_os("SWARMED_OPENCODE_CONFIG_DIR")
            .map_or_else(|| home.join(".config/opencode"), PathBuf::from);
        Ok(Self {
            data_dir,
            worktrees_root,
            claude_config,
            codex_config,
            opencode_config_dir,
        })
    }

    /// The SQLite database file.
    #[must_use]
    pub fn db_file(&self) -> PathBuf {
        self.data_dir.join("swarmed.db")
    }

    /// The file that lists the ACP agents sessions can run on, besides Claude Code.
    #[must_use]
    pub fn agents_config(&self) -> PathBuf {
        self.data_dir.join("agents.toml")
    }

    /// The file that lists the MCP servers set up in Swarmed itself.
    #[must_use]
    pub fn mcp_config(&self) -> PathBuf {
        self.data_dir.join("mcp.toml")
    }

    /// The file with the user's own preferences, such as their editor.
    #[must_use]
    pub fn settings(&self) -> PathBuf {
        self.data_dir.join("settings.toml")
    }

    /// The file with the user's custom layout.
    #[must_use]
    pub fn custom_layout(&self) -> PathBuf {
        self.data_dir.join("layout.json")
    }

    /// The user's own stylesheet, which the window applies after its own (`stylesheet.rs`).
    #[must_use]
    pub fn stylesheet(&self) -> PathBuf {
        self.data_dir.join("style.css")
    }

    /// The home of the built-in repolore: its configuration, index and model
    /// (`builtin_mcp.rs`).
    #[must_use]
    pub fn repolore_home(&self) -> PathBuf {
        self.data_dir.join("repolore")
    }

    /// The file with the user's configuration of the terminal.
    #[must_use]
    pub fn terminal_config(&self) -> PathBuf {
        self.data_dir.join("terminal.json")
    }

    /// The local config file of `project`.
    #[must_use]
    pub fn project_config(&self, project: &str) -> PathBuf {
        self.data_dir
            .join("projects")
            .join(format!("{project}.toml"))
    }

    /// The file holding the environments of `project`, beside its config.
    #[must_use]
    pub fn project_environments(&self, project: &str) -> PathBuf {
        self.data_dir
            .join("projects")
            .join(format!("{project}.environments.toml"))
    }

    /// The file holding the actions that belong to `project` alone, beside its config.
    #[must_use]
    pub fn project_actions(&self, project: &str) -> PathBuf {
        self.data_dir
            .join("projects")
            .join(format!("{project}.actions.toml"))
    }

    /// The directory holding the notes on `project` and on its tasks.
    #[must_use]
    pub fn notes(&self, project: &str) -> PathBuf {
        self.data_dir.join("notes").join(project)
    }

    /// The text file holding the user's note on `project`, or on its `task` when one is named.
    #[must_use]
    pub fn note(&self, project: &str, task: Option<&str>) -> PathBuf {
        let notes = self.notes(project);
        match task {
            Some(task) => notes.join("tasks").join(format!("{task}.txt")),
            None => notes.join("note.txt"),
        }
    }

    /// The socket this process answers its sessions' tool servers on.
    ///
    /// It is in the system's temporary folder and not in the data directory, because a socket's
    /// path may be about a hundred characters at most and a data directory can be deeper than
    /// that. The name carries the process, so the app and a command line running at once do
    /// not take each other's, and the data directory, so two cores in one process do not either.
    #[must_use]
    pub fn tools_socket(&self) -> PathBuf {
        let mut hasher = DefaultHasher::new();
        self.data_dir.hash(&mut hasher);
        let name = format!(
            "{SOCKET_PREFIX}{}-{:08x}.sock",
            std::process::id(),
            hasher.finish() & 0xffff_ffff
        );
        std::env::temp_dir().join(name)
    }

    /// The folder a session's tool server leaves the views it is given in.
    #[must_use]
    pub fn view_spool(&self, session: i64) -> PathBuf {
        self.data_dir.join("spool").join(session.to_string())
    }

    /// The folder that holds every session's images, pasted or returned by its tools, one
    /// folder per session.
    #[must_use]
    pub fn attachments_root(&self) -> PathBuf {
        self.data_dir.join("attachments")
    }

    /// The folder the images pasted into session `session`'s messages are kept in, and those
    /// its tools returned.
    #[must_use]
    pub fn attachments(&self, session: i64) -> PathBuf {
        self.attachments_root().join(session.to_string())
    }

    /// The folder that holds every task's checkpoints, one store each.
    #[must_use]
    pub fn checkpoints_root(&self) -> PathBuf {
        self.data_dir.join("checkpoints")
    }

    /// The store of the task with id `task`'s checkpoints: a bare git repository of
    /// Swarmed's own, which holds the trees of its worktree before and after each turn, and
    /// whatever of their files the task's repository has not got.
    #[must_use]
    pub fn checkpoints(&self, task: i64) -> PathBuf {
        self.checkpoints_root().join(task.to_string())
    }

    /// The folder that holds every task's browser, one each.
    #[must_use]
    pub fn browsers_root(&self) -> PathBuf {
        self.data_dir.join("browsers")
    }

    /// The folder of the browser of the task with id `task`: its Chrome's profile, which holds
    /// what was signed in to there, and what its pages downloaded. The user's alone.
    #[must_use]
    pub fn browser(&self, task: i64) -> PathBuf {
        self.browsers_root().join(task.to_string())
    }

    /// The worktree directory of `task` in `project`.
    #[must_use]
    pub fn worktree(&self, project: &str, task: &str) -> PathBuf {
        self.worktrees_root.join(project).join(task)
    }
}

/// The platform's place for application data: `~/Library/Application Support` on macOS,
/// `$XDG_DATA_HOME` or `~/.local/share` elsewhere.
fn default_data_dir(home: &Path) -> PathBuf {
    let base = if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map_or_else(|| home.join(".local").join("share"), PathBuf::from)
    };
    base.join("swarmed")
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.')
}

/// Project and task names become directory names, branch parts and Compose project names,
/// so they are restricted to a safe subset.
pub(crate) fn validate_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name.chars().all(is_name_char)
        && !name.ends_with('.')
        && !name.contains("..");
    if ok {
        Ok(())
    } else {
        Err(Error::InvalidName(name.to_string()))
    }
}

/// What a project's own files end in, before `.toml`, beside its config `<project>.toml`.
const PROJECT_FILES: [&str; 2] = [".actions", ".environments"];

/// A name a new project can have: a valid one that is not the name of another project's
/// file. Names may hold dots, and a project called `app.actions` would keep its config in
/// `app.actions.toml`, which is where the project `app` keeps its actions.
pub(crate) fn validate_project_name(name: &str) -> Result<()> {
    validate_name(name)?;
    if PROJECT_FILES.iter().any(|end| name.ends_with(end)) {
        return Err(Error::ReservedName(name.to_string()));
    }
    Ok(())
}

/// Turns a directory name into a valid project name, for the default of `project add`.
pub(crate) fn name_from_path(path: &Path) -> String {
    let raw = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().to_lowercase());
    let cleaned: String = raw
        .chars()
        .map(|c| if is_name_char(c) { c } else { '-' })
        .collect();
    cleaned
        .trim_matches(|c| matches!(c, '-' | '_' | '.'))
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_restricted_to_a_safe_subset() {
        for good in ["a", "fix-login", "pm-123", "v1.2", "a_b"] {
            assert!(validate_name(good).is_ok(), "{good:?} should be accepted");
        }
        for bad in [
            "",
            "Has Space",
            "../up",
            "UPPER",
            "-lead",
            "a/b",
            "end.",
            "a..b",
        ] {
            assert!(validate_name(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn a_project_cannot_be_named_like_another_projects_file() {
        for good in ["app", "app.web", "actions", "my.environments.app"] {
            assert!(validate_project_name(good).is_ok(), "{good:?}");
        }
        for bad in ["app.actions", "app.environments"] {
            assert!(
                matches!(validate_project_name(bad), Err(Error::ReservedName(_))),
                "{bad:?}"
            );
        }
        assert!(matches!(
            validate_project_name("Has Space"),
            Err(Error::InvalidName(_))
        ));
    }

    #[test]
    fn project_name_is_derived_from_the_directory() {
        assert_eq!(name_from_path(Path::new("/code/My Repo")), "my-repo");
        assert_eq!(name_from_path(Path::new("/code/.dotfiles")), "dotfiles");
    }
}

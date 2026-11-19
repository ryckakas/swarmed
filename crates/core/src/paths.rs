use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Where Swarmed keeps its own files. Nothing is ever written into a project's repo.
#[derive(Debug, Clone)]
pub struct Paths {
    /// Database and per-project config files.
    pub data_dir: PathBuf,
    /// Parent of all task worktrees: `<worktrees_root>/<project>/<task>`.
    pub worktrees_root: PathBuf,
    /// Claude Code's own config file, which is only ever read: it lists the user's MCP servers.
    pub claude_config: PathBuf,
}

impl Paths {
    /// The default locations, overridable with `SWARMED_DATA_DIR`, `SWARMED_WORKTREES_DIR` and
    /// `SWARMED_CLAUDE_CONFIG`.
    pub fn from_env() -> Result<Self> {
        let home = std::env::home_dir().ok_or(Error::NoHome)?;
        let data_dir = std::env::var_os("SWARMED_DATA_DIR")
            .map_or_else(|| default_data_dir(&home), PathBuf::from);
        let worktrees_root = std::env::var_os("SWARMED_WORKTREES_DIR")
            .map_or_else(|| home.join("swarmed"), PathBuf::from);
        let claude_config = std::env::var_os("SWARMED_CLAUDE_CONFIG")
            .map_or_else(|| home.join(".claude.json"), PathBuf::from);
        Ok(Self {
            data_dir,
            worktrees_root,
            claude_config,
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

    /// The file with the user's own preferences, such as their editor.
    #[must_use]
    pub fn settings(&self) -> PathBuf {
        self.data_dir.join("settings.toml")
    }

    /// The local config file of `project`.
    #[must_use]
    pub fn project_config(&self, project: &str) -> PathBuf {
        self.data_dir
            .join("projects")
            .join(format!("{project}.toml"))
    }

    /// The text file holding the user's note on `project`, or on its `task` when one is named.
    #[must_use]
    pub fn note(&self, project: &str, task: Option<&str>) -> PathBuf {
        let notes = self.data_dir.join("notes").join(project);
        match task {
            Some(task) => notes.join("tasks").join(format!("{task}.txt")),
            None => notes.join("note.txt"),
        }
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
    fn project_name_is_derived_from_the_directory() {
        assert_eq!(name_from_path(Path::new("/code/My Repo")), "my-repo");
        assert_eq!(name_from_path(Path::new("/code/.dotfiles")), "dotfiles");
    }
}

//! Snapshots: what a task's setup produced, kept so that the next task need not produce it
//! again.
//!
//! Setting a task up often means installing dependencies and building things, all of which
//! lands in files git ignores. A snapshot is a copy of exactly those files from one task
//! whose setup went well, one per environment of a project. A new task in that environment
//! gets them put into its fresh worktree before its `setup` runs, which then has little left
//! to do. Files git tracks are never part of a snapshot and never overwritten by one.
//!
//! The copies are made with `cp -c`, which on APFS clones files instead of copying them: a
//! snapshot and the worktrees started from it share their blocks until one of them changes,
//! so they take almost no room. What was built may hold the path of the worktree it was
//! built in; `setup` still runs afterwards and is what puts that right.
//!
//! Ignored files include the likes of `.env`, so a snapshot may hold credentials: its folder
//! is the user's alone, and what is in it is never logged.

use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::error::{Error, Result};
use crate::store::SetupStatus;
use crate::{Core, git, paths};

/// The folder a snapshot of tasks without an environment is kept under. No environment can
/// be called this: names never start with an underscore.
const NO_ENVIRONMENT: &str = "_default";
/// Inside a snapshot's folder: the files themselves, and what is known about them.
const FILES: &str = "files";
const ABOUT: &str = "snapshot.toml";

/// A snapshot as a front end shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// The environment whose tasks start from it; `None` for tasks without one.
    pub environment: Option<String>,
    /// The task it was taken from.
    pub task: String,
    /// When it was taken, in seconds since 1970.
    pub taken_at: u64,
    /// How many files and folders git ignores were kept, counted at the top of each.
    pub entries: usize,
}

fn failed(what: impl Into<String>) -> Error {
    Error::Snapshot(what.into())
}

impl Core {
    fn snapshot_folder(&self, project: &str, environment: Option<&str>) -> PathBuf {
        let snapshots = self.paths.data_dir.join("snapshots").join(project);
        snapshots.join(environment.unwrap_or(NO_ENVIRONMENT))
    }

    /// The project's snapshots, the one for tasks without an environment first.
    pub fn snapshots(&self, project: &str) -> Result<Vec<Snapshot>> {
        let project = self.project(project)?;
        let folder = self.paths.data_dir.join("snapshots").join(&project.name);
        let Ok(entries) = std::fs::read_dir(folder) else {
            return Ok(vec![]);
        };
        let read = |entry: std::fs::DirEntry| {
            let text = std::fs::read_to_string(entry.path().join(ABOUT)).ok()?;
            toml::from_str::<Snapshot>(&text).ok()
        };
        let mut snapshots: Vec<Snapshot> = entries.flatten().filter_map(read).collect();
        snapshots.sort_by(|a, b| a.environment.cmp(&b.environment));
        Ok(snapshots)
    }

    /// Keeps what the task's setup produced as the snapshot of its environment, in place of
    /// the one there was. Refused for a task whose setup did not go well: what a failed
    /// setup left is not worth starting from.
    pub async fn snapshot_save(&self, project: &str, task: &str) -> Result<Snapshot> {
        let (project, task) = self.task(project, task)?;
        if task.setup != SetupStatus::Ok {
            return Err(failed(format!(
                "`{}` was not set up successfully, so there is nothing good to keep",
                task.name
            )));
        }
        let ignored = ignored_in(&task.worktree_path).await?;
        if ignored.is_empty() {
            return Err(failed(format!(
                "setting `{}` up left no files that git ignores, so there is nothing to keep",
                task.name
            )));
        }
        let folder = self.snapshot_folder(&project.name, task.environment.as_deref());
        // Built beside the old one and swapped in, so a copy that fails leaves the old one.
        let building = folder.with_extension("new");
        remove(&building)?;
        private_dir(&building.join(FILES))?;
        for path in &ignored {
            copy_into(&task.worktree_path, path, &building.join(FILES)).await?;
        }
        let snapshot = Snapshot {
            environment: task.environment.clone(),
            task: task.name,
            taken_at: now(),
            entries: ignored.len(),
        };
        let about = toml::to_string(&snapshot).map_err(|error| failed(error.to_string()))?;
        std::fs::write(building.join(ABOUT), about)?;
        remove(&folder)?;
        std::fs::rename(&building, &folder)?;
        Ok(snapshot)
    }

    /// Forgets the snapshot of an environment, or of tasks without one.
    pub fn snapshot_remove(&self, project: &str, environment: Option<&str>) -> Result<()> {
        let project = self.project(project)?;
        if let Some(name) = environment {
            paths::validate_name(name)?;
        }
        remove(&self.snapshot_folder(&project.name, environment))
    }

    /// Puts the environment's snapshot into a fresh worktree, if there is one. Says whether
    /// it did. Nothing that is already there is overwritten, so what git checked out stays.
    pub(crate) async fn seed_from_snapshot(
        &self,
        project: &str,
        environment: Option<&str>,
        worktree: &Path,
    ) -> Result<bool> {
        let files = self.snapshot_folder(project, environment).join(FILES);
        if !files.is_dir() {
            return Ok(false);
        }
        // `/.` copies what is in the folder, not the folder; `-n` leaves what exists alone.
        let mut from = files.into_os_string();
        from.push("/.");
        cp(&["-cRpn"], Path::new(&from), worktree).await?;
        Ok(true)
    }
}

/// What git ignores in `worktree`, each file or folder named once at its top: a folder that
/// is ignored whole is one entry, however much is in it.
async fn ignored_in(worktree: &Path) -> Result<Vec<String>> {
    let listed = git::git(
        worktree,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
        ],
    )
    .await?;
    let paths = listed.lines().map(|line| line.trim_end_matches('/'));
    Ok(paths
        .filter(|path| !path.is_empty())
        .map(String::from)
        .collect())
}

/// Copies `path` of `worktree` to the same place under `target`.
async fn copy_into(worktree: &Path, path: &str, target: &Path) -> Result<()> {
    let to = target.join(path);
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    cp(&["-cRp"], &worktree.join(path), &to).await
}

/// Runs `cp`, cloning where the file system can. Its complaints are passed on without the
/// paths' contents ever being read here.
async fn cp(flags: &[&str], from: &Path, to: &Path) -> Result<()> {
    let output = Command::new("cp")
        .args(flags)
        .arg(from)
        .arg(to)
        .output()
        .await?;
    // With `-n`, BSD `cp` reports the files it left alone as a failure. They are the point.
    let skipping = flags.iter().any(|flag| flag.contains('n'));
    if output.status.success() || skipping {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&output.stderr);
    let first = said.lines().next().unwrap_or("cp failed");
    Err(failed(format!("could not copy: {first}")))
}

/// A folder only the user can look into, with the folders above it.
fn private_dir(folder: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true).mode(0o700).create(folder)?;
    Ok(())
}

fn remove(folder: &Path) -> Result<()> {
    match std::fs::remove_dir_all(folder) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

fn now() -> u64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH);
    since.map_or(0, |since| since.as_secs())
}

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

use std::collections::HashSet;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::changes::nul_separated;
use crate::error::{Error, Result};
use crate::store::SetupStatus;
use crate::{Core, git, paths};

/// The folder a snapshot of tasks without an environment is kept under. No environment can
/// be called this: names never start with an underscore.
const NO_ENVIRONMENT: &str = "_default";
/// What the name of a folder a snapshot is being built in starts with.
const BUILDING: &str = "_new.";
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
        // One that was being built when Swarmed ended is no snapshot.
        let whole =
            |entry: &std::fs::DirEntry| !entry.file_name().to_string_lossy().starts_with(BUILDING);
        let read = |entry: std::fs::DirEntry| {
            let text = std::fs::read_to_string(entry.path().join(ABOUT)).ok()?;
            toml::from_str::<Snapshot>(&text).ok()
        };
        let entries = entries.flatten().filter(whole);
        let mut snapshots: Vec<Snapshot> = entries.filter_map(read).collect();
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
        let building = building_beside(&folder);
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

    /// Forgets every snapshot of a project, along with the project.
    pub(crate) fn forget_snapshots(&self, project: &str) -> Result<()> {
        remove(&self.paths.data_dir.join("snapshots").join(project))
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
        cp(Path::new(&from), worktree, true).await?;
        Ok(true)
    }
}

/// Where a snapshot is built before it takes the place of the one in `folder`: beside it,
/// under a name no environment can have, since none starts with an underscore. Adding to the
/// name and not replacing what follows a dot in it matters: environments may have dots, and
/// `v1.2` must not build where `v1.3` does, nor `staging` in the folder of `staging.new`.
fn building_beside(folder: &Path) -> PathBuf {
    let name = folder.file_name().unwrap_or_default().to_string_lossy();
    folder.with_file_name(format!("{BUILDING}{name}"))
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
            "-z",
        ],
    )
    .await?;
    let paths = nul_separated(&listed).map(|path| path.trim_end_matches('/'));
    let paths: Vec<&str> = paths.filter(|path| !path.is_empty()).collect();
    Ok(outermost(&paths))
}

/// `paths` without those inside another of them. For a rule like `/vendor/*` git names the
/// folder and then everything in it as well. Copying both copies what is in it a second time,
/// into the copy already made, where it nests or fails when a file stands in the way.
fn outermost(paths: &[&str]) -> Vec<String> {
    let all: HashSet<&str> = paths.iter().copied().collect();
    // Each `/` in a path ends the name of a folder it is in.
    let inside_another = |path: &str| {
        path.match_indices('/')
            .any(|(at, _)| all.contains(&path[..at]))
    };
    paths
        .iter()
        .filter(|path| !inside_another(path))
        .map(|path| (*path).to_string())
        .collect()
}

/// Copies `path` of `worktree` to the same place under `target`.
async fn copy_into(worktree: &Path, path: &str, target: &Path) -> Result<()> {
    let to = target.join(path);
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    cp(&worktree.join(path), &to, false).await
}

/// How `cp` is told to clone where the file system can: the Mac's with `-c`, GNU's with
/// `--reflink=auto`. Neither knows the other's.
#[cfg(target_os = "macos")]
const CLONING: &str = "-c";
#[cfg(not(target_os = "macos"))]
const CLONING: &str = "--reflink=auto";

/// Runs `cp`, cloning where the file system can, and with `keep_existing` leaving what is
/// already at `to` as it is. Its complaints are passed on without the paths' contents ever
/// being read here.
async fn cp(from: &Path, to: &Path, keep_existing: bool) -> Result<()> {
    let mode = if keep_existing { "-Rpn" } else { "-Rp" };
    let output = Command::new("cp")
        .arg(CLONING)
        .arg(mode)
        .arg(from)
        .arg(to)
        .output()
        .await?;
    // With `-n`, `cp` may report the files it left alone as a failure. They are the point.
    if output.status.success() || keep_existing {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_lies_inside_another_entry_is_left_to_that_entry() {
        // As git lists what a `/vendor/*` rule ignores: the folder, then what is in it.
        let listed = [
            "vendor",
            "vendor/acme",
            "vendor/autoload.php",
            "vendor-bin",
            "web/dist",
            "web/dist/app.js",
            "webroot",
        ];
        assert_eq!(
            outermost(&listed),
            ["vendor", "vendor-bin", "web/dist", "webroot"]
        );
        // A name that only starts like a folder's is not inside it.
        assert_eq!(outermost(&["deps", "deps.lock"]), ["deps", "deps.lock"]);
    }
}

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
use crate::hooks::OutputSink;
use crate::store::{Project, SetupStatus, Task};
use crate::{Core, git, paths};

/// The folder a snapshot of tasks without an environment is kept under. No environment can
/// be called this: names never start with an underscore.
const NO_ENVIRONMENT: &str = "_default";
/// What the name of a folder a snapshot is being built in starts with.
const BUILDING: &str = "_new.";
/// What the name of the file whose lock is held while a snapshot is built starts with. Not a
/// folder, so it is never taken for a snapshot.
const LOCK: &str = "_lock.";
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
        let Some(_building) = building_lock(&folder)? else {
            return Err(failed(
                "a snapshot of this environment is being taken already; try again once it is kept",
            ));
        };
        build(&folder, &task, &ignored).await
    }

    /// [`Core::snapshot_first`], saying how it went. Not keeping one costs the task nothing:
    /// its next tasks are set up from nothing, as they always could be.
    pub(crate) async fn keep_first_snapshot(
        &self,
        project: &Project,
        task: &Task,
        sink: OutputSink<'_>,
    ) {
        match self.snapshot_first(project, task, sink).await {
            Ok(Some(_)) => sink("kept: new tasks of this environment start from it"),
            Ok(None) => {}
            Err(error) => sink(&format!("the snapshot could not be kept: {error}")),
        }
    }

    /// After a setup that went well: keeps what it produced as the snapshot of the task's
    /// environment, if the environment has none yet, so that its next tasks start from it.
    /// Says so first, since copying takes a while, once. `None` when there was one already,
    /// another is being taken, or setup left nothing git ignores.
    pub(crate) async fn snapshot_first(
        &self,
        project: &Project,
        task: &Task,
        sink: OutputSink<'_>,
    ) -> Result<Option<Snapshot>> {
        let folder = self.snapshot_folder(&project.name, task.environment.as_deref());
        let kept = || folder.join(ABOUT).is_file();
        if kept() {
            return Ok(None);
        }
        // Asked before the lock is: taking it makes its file, which a repository whose setup
        // leaves nothing to keep would get for nothing with every task.
        let ignored = ignored_in(&task.worktree_path).await?;
        if ignored.is_empty() {
            return Ok(None);
        }
        let Some(_building) = building_lock(&folder)? else {
            return Ok(None);
        };
        // Once more, now that no other can be taken: one may have been kept just before.
        if kept() {
            return Ok(None);
        }
        sink(
            "keeping what setup produced as the snapshot of this environment, for its next tasks to start from",
        );
        build(&folder, task, &ignored).await.map(Some)
    }

    /// Forgets every snapshot of a project, along with the project.
    pub(crate) fn forget_snapshots(&self, project: &str) -> Result<()> {
        remove(&self.paths.data_dir.join("snapshots").join(project))
    }

    /// Forgets the snapshot of an environment, or of tasks without one. Refused while one is
    /// being taken: it would be there again as soon as it is built.
    pub fn snapshot_remove(&self, project: &str, environment: Option<&str>) -> Result<()> {
        let project = self.project(project)?;
        if let Some(name) = environment {
            paths::validate_name(name)?;
        }
        let folder = self.snapshot_folder(&project.name, environment);
        let Some(_building) = building_lock(&folder)? else {
            return Err(failed(
                "a snapshot of this environment is being taken; forget it once it is kept",
            ));
        };
        remove(&folder)
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

/// Copies what git ignores in the task's worktree, `ignored`, as the snapshot in `folder`. It is
/// built beside the old one and swapped in, so a copy that fails leaves the old one. Only while
/// holding [`building_lock`] for `folder`.
async fn build(folder: &Path, task: &Task, ignored: &[String]) -> Result<Snapshot> {
    let building = building_beside(folder);
    remove(&building)?;
    private_dir(&building.join(FILES))?;
    for path in ignored {
        copy_into(&task.worktree_path, path, &building.join(FILES)).await?;
    }
    let snapshot = Snapshot {
        environment: task.environment.clone(),
        task: task.name.clone(),
        taken_at: now(),
        entries: ignored.len(),
    };
    let about = toml::to_string(&snapshot).map_err(|error| failed(error.to_string()))?;
    std::fs::write(building.join(ABOUT), about)?;
    remove(folder)?;
    std::fs::rename(&building, folder)?;
    Ok(snapshot)
}

/// The right to build a snapshot, held until it is dropped.
#[derive(Debug)]
struct Building(std::fs::File);

impl Drop for Building {
    fn drop(&mut self) {
        // Unlocked, not only closed: a program started meanwhile holds a copy of the file
        // until it runs, and the lock would stay with that copy, refusing the next build.
        let _unlocked: std::io::Result<()> = self.0.unlock();
    }
}

/// The right to build the snapshot in `folder`, for as long as what is returned is kept, or
/// `None` while another holds it. One build at a time, whether by this Swarmed or by another
/// process, such as the `swarmed` command: two would build in the same place.
fn building_lock(folder: &Path) -> Result<Option<Building>> {
    let name = folder.file_name().unwrap_or_default().to_string_lossy();
    let path = folder.with_file_name(format!("{LOCK}{name}"));
    if let Some(project) = path.parent() {
        private_dir(project)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(Building(file))),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(error)) => Err(error.into()),
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
    fn one_snapshot_of_an_environment_is_built_at_a_time() {
        let dir = tempfile::tempdir().expect("temp dir");
        let local = dir.path().join("demo").join("local");
        let first = building_lock(&local).expect("locks").expect("free");
        // Another build of the same environment is turned away.
        assert!(building_lock(&local).expect("asks").is_none());
        // Another environment's does not.
        let staging = building_lock(&dir.path().join("demo").join("staging"));
        assert!(staging.expect("asks").is_some());
        drop(first);
        assert!(building_lock(&local).expect("asks").is_some());
        // The lock is a file, so it is never listed as a snapshot of its own.
        assert!(dir.path().join("demo").join("_lock.local").is_file());
    }

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
        // Inside is at any depth, not only directly in the folder.
        assert_eq!(outermost(&["a", "a/b/c"]), ["a"]);
    }
}

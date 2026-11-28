//! Snapshots: what a task's setup produced, kept so that the next task need not produce it
//! again.
//!
//! Setting a task up often means installing dependencies and building things, all of which
//! lands in files git ignores. A snapshot is a copy of exactly those files from one task
//! whose setup went well, one per environment of a project. A new task in that environment
//! gets them put into its fresh worktree before its `setup` runs, which then has little left
//! to do. Files git tracks are never part of a snapshot and never overwritten by one.
//!
//! On APFS the copies are clones: a snapshot and the worktrees started from it share their
//! blocks until one of them changes, so they take almost no room. Each folder is cloned whole
//! in one call where it can be ([`clone_whole`]), else copied with `cp -c`, which clones one
//! file at a time. Both say how far they are, since a large one takes a while. What was built
//! may hold the path of the worktree it was built in; `setup` still runs afterwards and is
//! what puts that right.
//!
//! A snapshot is kept by the first task of its environment set up well, and by nothing else
//! unless the environment names the files its dependencies are pinned in (`snapshot_keys`,
//! such as `pnpm-lock.yaml`). Then a snapshot notes a fingerprint of them as they were in the
//! task it was taken from ([`taken_with`]), and a new task started from the base whose own
//! differ takes its place: the base has moved on, and every task after it would otherwise
//! reinstall what changed. Kept at the same moment as the first, after its setup and before
//! its stack writes logs and caches beside what setup made. A task started from another
//! branch, or on one that was there already, has that branch's dependencies, so it never takes
//! the place of one; it may still keep the first. The list says when a snapshot is behind the
//! base ([`Snapshot::behind`]).
//!
//! Ignored files include the likes of `.env`, so a snapshot may hold credentials: its folder
//! is the user's alone, and what is in it is never logged.

use std::collections::{BTreeMap, HashSet};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::changes::nul_separated;
use crate::clone::clone_whole;
use crate::config::ProjectConfig;
use crate::error::{Error, Result};
use crate::hooks::{self, OutputSink};
use crate::store::{Project, SetupStatus, Task};
use crate::tasks::base_of;
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
    /// Whether the files its environment's dependencies are pinned in (`snapshot_keys`) are
    /// no longer on the base branch as they were in the task it was taken from: the next task
    /// started from the base takes its place. Worked out as it is listed, never kept.
    #[serde(default)]
    pub behind: bool,
}

/// What is known about a snapshot, kept beside its files. A Swarmed that knows no fingerprint
/// reads past it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct About {
    pub(crate) environment: Option<String>,
    pub(crate) task: String,
    pub(crate) taken_at: u64,
    pub(crate) entries: usize,
    /// What it was taken with ([`taken_with`]): `None` when its environment named no files its
    /// dependencies are pinned in, or for one taken before they could be named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) fingerprint: Option<String>,
}

/// Why a task just set up well is kept as its environment's snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keeping {
    /// The environment has none yet.
    First,
    /// The one there is was taken with other dependencies than the task's, which came from
    /// the base.
    Newer,
}

impl Keeping {
    fn said(self) -> &'static str {
        match self {
            Self::First => {
                "keeping what setup produced as the snapshot of this environment, for its next tasks to start from"
            }
            Self::Newer => {
                "the snapshot of this environment was taken with other dependencies: keeping this task's in its place"
            }
        }
    }
}

/// Whether, and why, a task just set up well is kept as the snapshot in `folder`: when there
/// is none, or, if it may take the place of one (`newer`), when the one there was taken with
/// other dependencies than `fingerprint`, the task's.
fn keeping(folder: &Path, fingerprint: Option<&str>, newer: bool) -> Option<Keeping> {
    if !folder.join(ABOUT).is_file() {
        return Some(Keeping::First);
    }
    // One this Swarmed cannot read may be a newer Swarmed's: it is left to that one.
    let kept = read_about(folder)?;
    (newer && kept.fingerprint.as_deref() != fingerprint).then_some(Keeping::Newer)
}

/// One environment's snapshot as it lies on disk.
#[derive(Debug, Clone)]
pub(crate) struct SnapshotFolders {
    /// The environment; `None` for tasks without one.
    pub(crate) environment: Option<String>,
    /// The snapshot kept, if one is.
    pub(crate) snapshot: Option<About>,
    /// Its folders: the snapshot's, and the one a taking that was cut short left beside it.
    pub(crate) folders: Vec<PathBuf>,
}

/// Puts a folder of the snapshots' with the environment it is of: its snapshot's, or what a
/// taking of one that was cut short left, under a name of [`BUILDING`]'s.
fn file_folder(found: &mut BTreeMap<Option<String>, SnapshotFolders>, name: String, path: PathBuf) {
    let (own, unfinished) = match name.strip_prefix(BUILDING) {
        Some(rest) => (rest.to_string(), true),
        None => (name, false),
    };
    let environment = (own != NO_ENVIRONMENT).then_some(own);
    let listed = found
        .entry(environment.clone())
        .or_insert_with(|| SnapshotFolders {
            environment,
            snapshot: None,
            folders: vec![],
        });
    if !unfinished {
        listed.snapshot = read_about(&path);
    }
    listed.folders.push(path);
}

/// What is known about the snapshot in `folder`, if a whole one is there.
fn read_about(folder: &Path) -> Option<About> {
    let text = std::fs::read_to_string(folder.join(ABOUT)).ok()?;
    toml::from_str(&text).ok()
}

fn failed(what: impl Into<String>) -> Error {
    Error::Snapshot(what.into())
}

impl Core {
    fn snapshot_folder(&self, project: &str, environment: Option<&str>) -> PathBuf {
        let snapshots = self.paths.data_dir.join("snapshots").join(project);
        snapshots.join(environment.unwrap_or(NO_ENVIRONMENT))
    }

    /// The project's snapshots, the one for tasks without an environment first, each saying
    /// whether it is behind the base.
    pub async fn snapshots(&self, project: &str) -> Result<Vec<Snapshot>> {
        let project = self.project(project)?;
        let folder = self.paths.data_dir.join("snapshots").join(&project.name);
        let Ok(entries) = std::fs::read_dir(folder) else {
            return Ok(vec![]);
        };
        // One that was being built when Swarmed ended is no snapshot.
        let whole =
            |entry: &std::fs::DirEntry| !entry.file_name().to_string_lossy().starts_with(BUILDING);
        let entries = entries.flatten().filter(whole);
        let kept: Vec<About> = entries
            .filter_map(|entry| read_about(&entry.path()))
            .collect();
        let mut snapshots = Vec::with_capacity(kept.len());
        for about in kept {
            snapshots.push(self.shown(&project, about).await);
        }
        snapshots.sort_by(|a, b| a.environment.cmp(&b.environment));
        Ok(snapshots)
    }

    /// A kept snapshot as a front end shows it. Behind only when that can be told: a base or a
    /// file git cannot find makes no snapshot look behind.
    async fn shown(&self, project: &Project, about: About) -> Snapshot {
        let behind = self.behind_base(project, &about).await.unwrap_or(false);
        Snapshot {
            environment: about.environment,
            task: about.task,
            taken_at: about.taken_at,
            entries: about.entries,
            behind,
        }
    }

    /// Whether the snapshot was taken with other dependencies than the base branch has now:
    /// its fingerprint against the base's, in the repository. One taken without a fingerprint
    /// is behind as soon as its environment names files to take one of.
    async fn behind_base(&self, project: &Project, about: &About) -> Result<bool> {
        let config = self.config_in(&project.name, about.environment.as_deref())?;
        if config.snapshot_keys.is_empty() {
            return Ok(false);
        }
        let repo = project.repo()?;
        let base = base_of(repo, &config).await?;
        let now = taken_with(repo, &base, &config.snapshot_keys).await?;
        Ok(now != about.fingerprint)
    }

    /// Whether the task's branch was made for it from the project's base, rather than from
    /// another branch or checked out as it was: only then are the dependencies it was set up
    /// with those the next tasks start with. Not when that cannot be told, nor for a task made
    /// before Swarmed kept which it was.
    async fn started_from_base(
        &self,
        project: &Project,
        task: &Task,
        config: &ProjectConfig,
    ) -> bool {
        let made = self.store().task_branch_made(task.id);
        let (Ok(Some(true)), Ok(repo)) = (made, project.repo()) else {
            return false;
        };
        base_of(repo, config)
            .await
            .is_ok_and(|base| base == task.base_ref)
    }

    /// Keeps what the task's setup produced as the snapshot of its environment, in place of
    /// the one there was. Refused for a task whose setup did not go well: what a failed
    /// setup left is not worth starting from.
    pub async fn snapshot_save(&self, project: &str, task: &str) -> Result<Snapshot> {
        let (project, task) = self.repo_task(project, task)?;
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
        let config = self.config_in(&project.name, task.environment.as_deref())?;
        let fingerprint = taken_with(&task.worktree_path, "HEAD", &config.snapshot_keys).await?;
        let folder = self.snapshot_folder(&project.name, task.environment.as_deref());
        let Some(building) = building_lock(&folder)? else {
            return Err(failed(
                "a snapshot of this environment is being taken already; try again once it is kept",
            ));
        };
        let about = build(&folder, &task, &ignored, fingerprint, &hooks::discard).await?;
        // Let go before the base is asked about, which another taking need not wait for.
        drop(building);
        Ok(self.shown(&project, about).await)
    }

    /// [`Core::snapshot_after_setup`], saying how it went. Not keeping one costs the task
    /// nothing: its next tasks start from the snapshot there was, or from nothing, as they
    /// always could.
    pub(crate) async fn keep_snapshot(&self, project: &Project, task: &Task, sink: OutputSink<'_>) {
        let began = Instant::now();
        match self.snapshot_after_setup(project, task, sink).await {
            Ok(Some(_)) => sink(&format!(
                "kept: new tasks of this environment start from it (copied in {})",
                seconds(began.elapsed())
            )),
            Ok(None) => {}
            Err(error) => sink(&format!("the snapshot could not be kept: {error}")),
        }
    }

    /// After a setup that went well: keeps what it produced as the snapshot of the task's
    /// environment, so that its next tasks start from it, if the environment has none yet, or
    /// if the task started from the base with other dependencies than the snapshot was taken
    /// with ([`keeping`]). Says why first, since copying takes a while, once. `None` when it
    /// is not kept, another is being taken, or setup left nothing git ignores.
    pub(crate) async fn snapshot_after_setup(
        &self,
        project: &Project,
        task: &Task,
        sink: OutputSink<'_>,
    ) -> Result<Option<About>> {
        let folder = self.snapshot_folder(&project.name, task.environment.as_deref());
        let config = self.config_in(&project.name, task.environment.as_deref())?;
        let fingerprint = taken_with(&task.worktree_path, "HEAD", &config.snapshot_keys).await?;
        let newer = fingerprint.is_some() && self.started_from_base(project, task, &config).await;
        let why = || keeping(&folder, fingerprint.as_deref(), newer);
        if why().is_none() {
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
        let Some(why) = why() else {
            return Ok(None);
        };
        sink(why.said());
        build(&folder, task, &ignored, fingerprint, sink)
            .await
            .map(Some)
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
        // What a taking that was cut short left goes too: with the lock free, nothing is
        // building there, and the next taking would only delete it first.
        remove(&building_beside(&folder))?;
        remove(&folder)
    }

    /// Each environment's snapshot of a project as it lies on disk, for a clean-up to list:
    /// the one of tasks without an environment first. An environment is listed when it has a
    /// snapshot, or only what a taking that was cut short left.
    pub(crate) fn snapshot_folders(&self, project: &str) -> Vec<SnapshotFolders> {
        let folder = self.paths.data_dir.join("snapshots").join(project);
        let Ok(entries) = std::fs::read_dir(folder) else {
            return vec![];
        };
        let mut found: BTreeMap<Option<String>, SnapshotFolders> = BTreeMap::new();
        // The lock files lie beside the folders, and are not folders.
        let folders = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()));
        for entry in folders {
            let name = entry.file_name().to_string_lossy().into_owned();
            file_folder(&mut found, name, entry.path());
        }
        found.into_values().collect()
    }

    /// Puts the environment's snapshot into a fresh worktree, if there is one, saying which
    /// part it is at, and how long it took once done; `None` without a snapshot. Nothing that
    /// is already there is overwritten, so what git checked out stays.
    pub(crate) async fn seed_from_snapshot(
        &self,
        project: &str,
        environment: Option<&str>,
        worktree: &Path,
        sink: OutputSink<'_>,
    ) -> Result<Option<Duration>> {
        let files = self.snapshot_folder(project, environment).join(FILES);
        if !files.is_dir() {
            return Ok(None);
        }
        let began = Instant::now();
        let missing = missing_from(&files, worktree)?;
        for (at, path) in missing.iter().enumerate() {
            let shown = path.display();
            let of = missing.len();
            sink(&format!(
                "starting from the snapshot of this environment: {} of {of}, {shown}",
                at + 1
            ));
            put(&files.join(path), &worktree.join(path), true).await?;
        }
        Ok(Some(began.elapsed()))
    }
}

/// A length of time as the setup output gives it: `7.2 s`.
pub(crate) fn seconds(took: Duration) -> String {
    format!("{:.1} s", took.as_secs_f64())
}

/// How the worktree stands where the snapshot has something.
enum There {
    /// Not there: it is put there whole.
    Missing,
    /// A folder in both, as one git tracks files in: what is in it is looked at in turn.
    Folder,
    /// There already, and left as it is.
    Kept,
}

fn there(ours: &Path, theirs: &Path) -> There {
    let Ok(theirs) = std::fs::symlink_metadata(theirs) else {
        return There::Missing;
    };
    let ours_a_folder = std::fs::symlink_metadata(ours).is_ok_and(|ours| ours.is_dir());
    if theirs.is_dir() && ours_a_folder {
        There::Folder
    } else {
        There::Kept
    }
}

/// What of the snapshot's `files` the worktree does not have yet, each named once at its top:
/// a folder the worktree lacks is one entry, however much is in it, so that it can be cloned
/// in one go. In order, so that how far it is can be said.
fn missing_from(files: &Path, worktree: &Path) -> Result<Vec<PathBuf>> {
    let mut missing = Vec::new();
    let mut folders = vec![PathBuf::new()];
    while let Some(folder) = folders.pop() {
        let names =
            std::fs::read_dir(files.join(&folder))?.map(|entry| entry.map(|e| e.file_name()));
        for name in names {
            let path = folder.join(name?);
            match there(&files.join(&path), &worktree.join(&path)) {
                There::Missing => missing.push(path),
                There::Folder => folders.push(path),
                There::Kept => (),
            }
        }
    }
    missing.sort();
    Ok(missing)
}

/// Puts `from` at `to`, where nothing is yet: cloned whole where the file system can, else
/// copied with `cp`, leaving what is at `to` alone if `keep_existing`.
async fn put(from: &Path, to: &Path, keep_existing: bool) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let (source, target) = (from.to_path_buf(), to.to_path_buf());
    // Off the async threads: a large folder takes seconds even as one clone.
    let cloned = tokio::task::spawn_blocking(move || clone_whole(&source, &target)).await;
    if cloned.unwrap_or(false) {
        return Ok(());
    }
    cp(from, to, keep_existing).await
}

/// Copies what git ignores in the task's worktree, `ignored`, as the snapshot in `folder`,
/// noting the `fingerprint` it was taken with, saying which part it is at. It is built beside
/// the old one and swapped in, so a copy that fails leaves the old one. Only while holding
/// [`building_lock`] for `folder`.
async fn build(
    folder: &Path,
    task: &Task,
    ignored: &[String],
    fingerprint: Option<String>,
    sink: OutputSink<'_>,
) -> Result<About> {
    let building = building_beside(folder);
    remove(&building)?;
    let files = building.join(FILES);
    private_dir(&files)?;
    for (at, path) in ignored.iter().enumerate() {
        let of = ignored.len();
        sink(&format!(
            "keeping what setup produced as the snapshot of this environment: {} of {of}, {path}",
            at + 1
        ));
        put(&task.worktree_path.join(path), &files.join(path), false).await?;
    }
    let about = About {
        environment: task.environment.clone(),
        task: task.name.clone(),
        taken_at: now(),
        entries: ignored.len(),
        fingerprint,
    };
    let written = toml::to_string(&about).map_err(|error| failed(error.to_string()))?;
    std::fs::write(building.join(ABOUT), written)?;
    remove(folder)?;
    std::fs::rename(&building, folder)?;
    Ok(about)
}

/// What a snapshot taken at `commit` of the repository or worktree `dir` is taken with: a
/// fingerprint of the files `keys` names there, the tracked files the dependencies are pinned
/// in. It hashes what `git ls-tree` lists, each file's blob as git keeps it, so no file is read.
/// One that is not there is not listed, so adding it later changes the fingerprint too. `None`
/// without keys.
async fn taken_with(dir: &Path, commit: &str, keys: &[String]) -> Result<Option<String>> {
    if keys.is_empty() {
        return Ok(None);
    }
    // From the top of the tree whatever folder `dir` is, and in its order whatever the keys'.
    let mut args = vec!["ls-tree", "-z", "--full-tree", commit, "--"];
    args.extend(keys.iter().map(String::as_str));
    let listed = git::git(dir, &args).await?;
    Ok(Some(format!("{:016x}", fnv1a(listed.as_bytes()))))
}

/// The 64-bit FNV-1a hash of `bytes`. The same in every Swarmed, which the standard library's
/// hasher is not from one release of Rust to the next: a fingerprint one kept is compared by
/// others sharing the data directory.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
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
    fn a_fingerprint_is_hashed_the_same_way_by_every_swarmed() {
        // FNV-1a's published values: a change of hash would make every snapshot look behind.
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn a_snapshot_is_kept_again_only_when_it_may_be_and_was_taken_with_other_dependencies() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = dir.path().join("local");
        // None yet: whoever comes first keeps one, from the base or not.
        assert_eq!(keeping(&folder, None, false), Some(Keeping::First));
        assert_eq!(keeping(&folder, Some("1"), false), Some(Keeping::First));

        let kept = |fingerprint: Option<&str>| {
            let about = About {
                environment: None,
                task: "one".to_string(),
                taken_at: 1,
                entries: 1,
                fingerprint: fingerprint.map(str::to_string),
            };
            std::fs::create_dir_all(&folder).expect("test setup");
            let written = toml::to_string(&about).expect("writes");
            std::fs::write(folder.join(ABOUT), written).expect("test setup");
        };
        kept(Some("1"));
        assert_eq!(keeping(&folder, Some("1"), true), None);
        assert_eq!(keeping(&folder, Some("2"), true), Some(Keeping::Newer));
        // Not from the base, a task's other dependencies are its branch's.
        assert_eq!(keeping(&folder, Some("2"), false), None);
        // Taken before there was a fingerprint, it is behind any task's.
        kept(None);
        assert_eq!(keeping(&folder, Some("1"), true), Some(Keeping::Newer));
        // One this Swarmed cannot read is left to whoever wrote it.
        std::fs::write(folder.join(ABOUT), "taken_at = \"soon\"").expect("test setup");
        assert_eq!(keeping(&folder, Some("1"), true), None);
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

    fn package(files: &Path, name: &str) {
        let folder = files.join("node_modules").join(name);
        std::fs::create_dir_all(&folder).expect("test setup");
        std::fs::write(folder.join("lib.js"), name).expect("test setup");
    }

    #[tokio::test]
    async fn an_empty_folder_in_the_worktree_is_filled_part_by_part() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (files, worktree) = (dir.path().join("files"), dir.path().join("worktree"));
        package(&files, "acme");
        package(&files, "beta");
        std::fs::create_dir_all(worktree.join("node_modules")).expect("test setup");

        // The folder is there, so it is looked into, and each package is a part of its own.
        let missing = missing_from(&files, &worktree).expect("reads");
        let [acme, beta] = ["node_modules/acme", "node_modules/beta"].map(PathBuf::from);
        assert_eq!(missing, [acme.clone(), beta.clone()]);
        let placed = put(&files.join(&acme), &worktree.join(&acme), true).await;
        placed.expect("puts");
        let placed = put(&files.join(&beta), &worktree.join(&beta), true).await;
        placed.expect("puts");

        // The folder was filled where it stood, and nothing landed inside itself.
        let modules = worktree.join("node_modules");
        let read = |path: &str| std::fs::read_to_string(modules.join(path)).expect("there");
        assert_eq!(read("acme/lib.js"), "acme");
        assert_eq!(read("beta/lib.js"), "beta");
        let mut names: Vec<_> = std::fs::read_dir(&modules)
            .expect("reads")
            .map(|entry| entry.expect("an entry").file_name())
            .collect();
        names.sort();
        assert_eq!(names, ["acme", "beta"]);
        assert!(!modules.join("acme/acme").exists());
        // Nothing is left to put a second time.
        let left = missing_from(&files, &worktree).expect("reads");
        assert_eq!(left, Vec::<PathBuf>::new());
    }
}

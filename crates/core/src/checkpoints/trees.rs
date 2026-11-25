//! Checkpoints as git trees: taken from a worktree into a store of objects of their own, and
//! compared, without writing anything into the repository.
//!
//! Git is pointed at the repository for everything it reads (what it ignores, its attributes,
//! the objects it has) and at the store for every object it writes, with an index of its own:
//! a copy of the worktree's, whose stat cache spares hashing what did not change. No ref, no
//! object and no index of the repository's is written.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::changes::{self, FileChange, nul_separated};
use crate::error::{Error, Result};

/// Files git does not track that are larger than this, in bytes, are left out of a checkpoint:
/// 10 MB, as sizes are shown everywhere in Swarmed.
pub const MAX_UNTRACKED_BYTES: u64 = 10_000_000;

/// How long one checkpoint, or one reading of them, may take before it is given up: a message
/// never waits on one longer.
const LIMIT: Duration = Duration::from_secs(30);

/// How long git may take to write a worktree's files once it has begun: stopped at it, it
/// leaves them part-way, so it is given far longer than a reading.
const WRITE_LIMIT: Duration = Duration::from_secs(600);

/// How often a git that is still at work is looked at.
const POLL: Duration = Duration::from_millis(2);

/// How old an object of a store has to be before a prune deletes it: a checkpoint taken
/// meanwhile writes objects that no row names yet.
const GRACE: &str = "1.hour.ago";

/// What git is told for a checkpoint whatever the repository says: run no hooks, start no
/// file-system monitor, write the index whole, and say nothing about repositories inside it.
/// The hooks are looked for where there are none.
const SETTINGS: [(&str, &str); 4] = [
    ("core.fsmonitor", "false"),
    ("core.splitIndex", "false"),
    ("advice.addEmbeddedRepo", "false"),
    ("gc.auto", "0"),
];

/// A file left out of a checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedFile {
    /// The path, relative to the worktree.
    pub path: String,
    /// Its size, in bytes.
    pub bytes: u64,
}

/// A checkpoint just taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Taken {
    /// The tree's id.
    pub(crate) tree: String,
    /// The files left out of it.
    pub(crate) skipped: Vec<SkippedFile>,
    /// The commit the worktree was on, or `None` on a branch with no commits yet.
    pub(crate) head: Option<String>,
    /// How long it took, in milliseconds.
    pub(crate) took_ms: u64,
}

/// The worktree's files as a tree in `store`: tracked and untracked files, as `git add -A`
/// sees them, but the untracked ones over [`MAX_UNTRACKED_BYTES`].
pub(crate) fn take(worktree: &Path, store: &Path) -> Result<Taken> {
    let started = Instant::now();
    let deadline = started + LIMIT;
    let repository = Repository::of(worktree, deadline)?;
    let taking = Taking::begin(worktree, store, &repository, deadline)?;
    let taken = taking.taken(started)?;
    Ok(taken)
}

/// Takes the worktree back to tree `to`, a checkpoint of it: what differs from it is written as
/// it is there, and what it has not got is deleted. What git ignores, and untracked files too
/// large to keep, are neither written nor deleted: one in the way of a file of `to` refuses the
/// whole before anything is changed ([`nothing_in_the_way`]), since it is in no tree and could
/// not be put back. The worktree as it was is taken first and returned, so that it can be
/// put back. With `only_from`, nothing is done unless the worktree is at that tree now: one
/// that changed since would lose what changed. Refused in the middle of a rebase or a merge.
pub(crate) fn restore(
    worktree: &Path,
    store: &Path,
    to: &str,
    only_from: Option<&str>,
) -> std::result::Result<Taken, Stopped> {
    let started = Instant::now();
    let deadline = started + LIMIT;
    let repository = Repository::of(worktree, deadline)?;
    quiet(&repository)?;
    let mut taking = Taking::begin(worktree, store, &repository, deadline)?;
    let was = taking.taken(started)?;
    if only_from.is_some_and(|tree| tree != was.tree) {
        let changed = "the files changed since they were taken back: putting them back would \
                       lose that";
        return Err(Error::Rewind(changed.to_string()).into());
    }
    nothing_in_the_way(&taking.git, &was.tree, to)?;
    let Err(why) = taking.write(&was.tree, to) else {
        return Ok(was);
    };
    drop(taking);
    // Git writes file by file, and may have written some: they are put as they were.
    let now = left_at(worktree, store, &repository, &was.tree);
    let left = now.ok().flatten().map(|now| Left { was: was.tree, now });
    Err(Stopped { why, left })
}

/// Why a worktree's files were not taken to another tree.
#[derive(Debug)]
pub(crate) struct Stopped {
    /// What refused it, or what git said.
    pub(crate) why: Error,
    /// What became of the files, if git gave up part-way and they could not be put as they
    /// were either. Otherwise they are as they were.
    pub(crate) left: Option<Left>,
}

/// A worktree whose files are part-way between two trees.
#[derive(Debug)]
pub(crate) struct Left {
    /// The worktree as it was before, which nothing else names.
    pub(crate) was: String,
    /// The worktree as it is left.
    pub(crate) now: String,
}

impl From<Error> for Stopped {
    fn from(why: Error) -> Self {
        Self { why, left: None }
    }
}

/// The tree the worktree is left at after git gave up taking it from tree `was` to another,
/// once what it wrote by then has been put as it was: `None` when that worked, or git had
/// written nothing.
fn left_at(
    worktree: &Path,
    store: &Path,
    repository: &Repository,
    was: &str,
) -> Result<Option<String>> {
    let started = Instant::now();
    // An index of its own: the one git gave up on may be left locked.
    let mut taking = Taking::begin(worktree, store, repository, started + LIMIT)?;
    let now = taking.taken(started)?.tree;
    if now == was {
        return Ok(None);
    }
    let undone = nothing_in_the_way(&taking.git, &now, was).and_then(|()| taking.write(&now, was));
    Ok(undone.is_err().then_some(now))
}

/// Refuses if something on disk that tree `was`, the worktree as taken, has not got lies where
/// going to tree `to` would write: a file git ignores, or an untracked one too large to keep.
/// Git itself refuses only for the latter, and writes over what it ignores without a word.
fn nothing_in_the_way(git: &Git<'_>, was: &str, to: &str) -> Result<()> {
    let kinds = git.text(&["diff", "--no-renames", "--name-status", "-z", was, to])?;
    let on_disk = |path: &str| {
        let found = std::fs::symlink_metadata(git.worktree.join(path)).ok()?;
        Some(found.is_dir())
    };
    match in_the_way(&kinds, on_disk) {
        Some(path) => Err(Error::Rewind(format!(
            "{path} is in the way of a file that would come back, and is not kept in any \
             checkpoint (git ignores it, or it is too large): move it away first"
        ))),
        None => Ok(()),
    }
}

/// The first path that `kinds`, git's `--name-status -z` from one tree to another, adds where
/// something is on disk already. `on_disk` says of a path whether it is there, and a folder.
fn in_the_way(kinds: &str, on_disk: impl Fn(&str) -> Option<bool>) -> Option<String> {
    let mut parts = nul_separated(kinds);
    let (mut added, mut gone) = (vec![], BTreeSet::new());
    while let (Some(kind), Some(path)) = (parts.next(), parts.next()) {
        match kind {
            "A" => added.push(path),
            "D" => drop(gone.insert(path.to_lowercase())),
            _ => {}
        }
    }
    let blocked = |path: &&str| {
        // The same name in another case is the same file where the file system tells no
        // case apart: it is in the first tree, and goes with it.
        if gone.contains(&path.to_lowercase()) {
            return false;
        }
        // A file where a folder of the path has to be, unless the first tree has it.
        let folders = path.match_indices('/').map(|(at, _)| &path[..at]);
        let file_above =
            |folder: &str| on_disk(folder) == Some(false) && !gone.contains(&folder.to_lowercase());
        on_disk(path).is_some() || { folders }.any(file_above)
    };
    added.into_iter().find(blocked).map(str::to_string)
}

/// Refuses while git is in the middle of something in the worktree, which its files must not
/// be changed under.
pub(crate) fn not_mid_operation(worktree: &Path) -> Result<()> {
    quiet(&Repository::of(worktree, Instant::now() + LIMIT)?)
}

fn quiet(repository: &Repository) -> Result<()> {
    match under_way(&repository.git_dir) {
        Some(what) => Err(Error::Rewind(format!(
            "{what} is under way in this worktree: finish it or abort it first"
        ))),
        None => Ok(()),
    }
}

/// What is under way in the worktree whose own folder in the repository is `git_dir`, if
/// anything git keeps state for: files must not be changed under it.
fn under_way(git_dir: &Path) -> Option<&'static str> {
    let states = [
        ("rebase-merge", "a rebase"),
        ("rebase-apply", "a rebase"),
        ("MERGE_HEAD", "a merge"),
        ("CHERRY_PICK_HEAD", "a cherry-pick"),
        ("REVERT_HEAD", "a revert"),
    ];
    states
        .iter()
        .find(|(file, _)| git_dir.join(file).exists())
        .map(|(_, what)| *what)
}

/// How many commits the worktree's branch has that commit `then` had not: those made since.
/// `None` when it cannot be told, such as for a commit that is gone.
pub(crate) fn commits_since(worktree: &Path, then: &str) -> Option<usize> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(worktree)
        .args(["rev-list", "--count", &format!("{then}..HEAD"), "--"])
        .env("GIT_TERMINAL_PROMPT", "0");
    let output = run(command, None, Instant::now() + LIMIT).ok()?;
    succeeded("rev-list", &output).ok()?.trim().parse().ok()
}

/// A tree being taken of a worktree: the index it is built in, and how git is run for it.
struct Taking<'a> {
    git: Git<'a>,
    /// Held while the index is in use, and deleted with it.
    _index: Scratch,
}

impl<'a> Taking<'a> {
    fn begin(
        worktree: &'a Path,
        store: &'a Path,
        repository: &Repository,
        deadline: Instant,
    ) -> Result<Self> {
        open_store(store, deadline)?;
        let index = Scratch::index(store, &repository.git_dir)?;
        let mut git = Git::new(worktree, store, repository, deadline);
        git.config = settings(worktree, store, deadline)?;
        git.index = Some(index.0.clone());
        Ok(Self { git, _index: index })
    }

    /// The worktree's files as a tree, in the index, which then holds them.
    fn taken(&self, started: Instant) -> Result<Taken> {
        let skipped = too_large(&self.git)?;
        self.git.add_all(&skipped)?;
        let tree = self.git.line(&["write-tree"])?;
        let head = self.head();
        let took = started.elapsed().as_millis();
        Ok(Taken {
            tree,
            skipped,
            head,
            took_ms: u64::try_from(took).unwrap_or(u64::MAX),
        })
    }

    /// Takes the worktree's files from tree `from`, which the index holds, to tree `to`. Given
    /// [`WRITE_LIMIT`] of its own, however long taking the worktree took: a git stopped while
    /// it writes leaves the files part-way.
    fn write(&mut self, from: &str, to: &str) -> Result<()> {
        // Submodules are left as they are: what is in them is no part of a checkpoint.
        let submodules = ("submodule.recurse".to_string(), "false".to_string());
        self.git.config.push(submodules);
        self.git.deadline = Instant::now() + WRITE_LIMIT;
        let written = self.git.text(&["read-tree", "-m", "-u", from, to]);
        written.map(drop).map_err(|error| match error {
            Error::Git { stderr, .. } => {
                Error::Rewind(format!("git would not take the files back: {stderr}"))
            }
            other => other,
        })
    }

    /// The commit the worktree is on; `None` on a branch with no commits yet.
    fn head(&self) -> Option<String> {
        let output = run(
            self.git
                .command(&["rev-parse", "--quiet", "--verify", "HEAD^{commit}"]),
            None,
            self.git.deadline,
        )
        .ok()?;
        let printed = succeeded("rev-parse", &output).ok()?;
        Some(printed.trim().to_string()).filter(|head| !head.is_empty())
    }
}

/// The files that differ from tree `from` to tree `to`, as the task's changes are listed.
pub(crate) fn changed(
    worktree: &Path,
    store: &Path,
    (from, to): (&str, &str),
) -> Result<Vec<FileChange>> {
    let git = Git::reading(worktree, store)?;
    let kinds = git.text(&["diff", "--no-renames", "--name-status", "-z", from, to])?;
    let counts = git.text(&["diff", "--no-renames", "--numstat", "-z", from, to])?;
    Ok(changes::tracked_changes(&kinds, &counts)
        .into_iter()
        .map(changes::file_change)
        .collect())
}

/// How one file differs from tree `from` to tree `to`, as a unified diff.
pub(crate) fn diff(
    worktree: &Path,
    store: &Path,
    (from, to): (&str, &str),
    path: &str,
) -> Result<String> {
    let git = Git::reading(worktree, store)?;
    git.text(&[
        "diff",
        "--no-renames",
        "--no-ext-diff",
        "--no-color",
        from,
        to,
        "--",
        path,
    ])
}

/// Deletes the objects of `store` that none of the trees in `keep` needs and that are older
/// than [`GRACE`]. The objects of the repository are only read.
pub(crate) fn prune(worktree: &Path, store: &Path, keep: &[String]) -> Result<()> {
    let deadline = Instant::now() + LIMIT;
    let repository = Repository::of(worktree, deadline)?;
    let mut command = Command::new("git");
    command
        .arg("--git-dir")
        .arg(store)
        .args(["prune", &format!("--expire={GRACE}"), "--"])
        .args(keep)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env(
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            alternate(&repository.objects),
        );
    let output = run(command, None, deadline)?;
    succeeded("prune", &output).map(drop)
}

/// The folders git keeps a worktree's repository in.
struct Repository {
    /// The worktree's own folder in the repository, which holds its index.
    git_dir: PathBuf,
    /// The repository's objects, shared by all its worktrees.
    objects: PathBuf,
}

impl Repository {
    fn of(worktree: &Path, deadline: Instant) -> Result<Self> {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(worktree)
            .args([
                "rev-parse",
                "--path-format=absolute",
                "--git-dir",
                "--git-common-dir",
            ])
            .env("GIT_TERMINAL_PROMPT", "0");
        let printed = succeeded("rev-parse", &run(command, None, deadline)?)?;
        let mut lines = printed.lines();
        let (Some(git_dir), Some(common)) = (lines.next(), lines.next()) else {
            return Err(Error::NotARepo(worktree.to_path_buf()));
        };
        Ok(Self {
            git_dir: PathBuf::from(git_dir),
            objects: Path::new(common).join("objects"),
        })
    }
}

/// Makes `store` a bare repository of its own if it is not one yet: the objects go into its
/// `objects`, and a prune needs a repository to run in. It is given nothing of the user's
/// templates.
fn open_store(store: &Path, deadline: Instant) -> Result<()> {
    if store.join("HEAD").exists() {
        return Ok(());
    }
    if let Some(parent) = store.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut command = Command::new("git");
    command
        .args(["init", "--bare", "--quiet", "--template="])
        .arg(store)
        .env("GIT_TERMINAL_PROMPT", "0");
    succeeded("init", &run(command, None, deadline)?).map(drop)
}

/// A copy of the worktree's index in the store, for one checkpoint, deleted when it is done
/// with. The index git keeps for the worktree is only read.
struct Scratch(PathBuf);

impl Scratch {
    fn index(store: &Path, git_dir: &Path) -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = store.join(format!("index-{}-{number}", std::process::id()));
        let index = git_dir.join("index");
        // A worktree with no index yet starts from none: every file is then hashed.
        match std::fs::copy(&index, &path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error.into());
            }
            Err(_) => {}
            Ok(_) => as_old_as(&index, &path)?,
        }
        Ok(Self(path))
    }
}

/// Gives `copy` the time `index` was last written. Git takes a file as unchanged when its
/// time and size are as the index has them, unless the file is as new as the index itself,
/// when it reads it. A copy keeps the time on a Mac and gets a new one on Linux, where a file
/// changed in the moment the index was written would then be passed over.
fn as_old_as(index: &Path, copy: &Path) -> Result<()> {
    let written = std::fs::metadata(index)?.modified()?;
    let copy = std::fs::File::options().write(true).open(copy)?;
    copy.set_modified(written)?;
    Ok(())
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).ok();
        let mut lock = self.0.clone().into_os_string();
        lock.push(".lock");
        std::fs::remove_file(lock).ok();
    }
}

/// How git is run on a worktree for its checkpoints.
struct Git<'a> {
    worktree: &'a Path,
    store: &'a Path,
    /// The repository's objects, read where the store has not got them.
    alternates: String,
    /// The index git works on, if it needs one: never the worktree's own.
    index: Option<PathBuf>,
    /// Settings that stand in for the repository's.
    config: Vec<(String, String)>,
    /// Whether paths are taken as they are written, with no patterns or magic in them.
    literal: bool,
    deadline: Instant,
}

impl<'a> Git<'a> {
    fn new(
        worktree: &'a Path,
        store: &'a Path,
        repository: &Repository,
        deadline: Instant,
    ) -> Self {
        Self {
            worktree,
            store,
            alternates: alternate(&repository.objects),
            index: None,
            config: vec![],
            literal: false,
            deadline,
        }
    }

    /// Git for reading checkpoints: no index, and paths taken as they are written.
    fn reading(worktree: &'a Path, store: &'a Path) -> Result<Self> {
        let deadline = Instant::now() + LIMIT;
        let repository = Repository::of(worktree, deadline)?;
        let mut git = Self::new(worktree, store, &repository, deadline);
        git.literal = true;
        Ok(git)
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(self.worktree)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_OBJECT_DIRECTORY", self.store.join("objects"))
            .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", &self.alternates)
            .env("GIT_CONFIG_COUNT", self.config.len().to_string());
        for (at, (key, value)) in self.config.iter().enumerate() {
            command
                .env(format!("GIT_CONFIG_KEY_{at}"), key)
                .env(format!("GIT_CONFIG_VALUE_{at}"), value);
        }
        match &self.index {
            Some(index) => command.env("GIT_INDEX_FILE", index),
            None => command.env_remove("GIT_INDEX_FILE"),
        };
        if self.literal {
            command.env("GIT_LITERAL_PATHSPECS", "1");
        }
        command
    }

    /// What git printed, as it printed it.
    fn text(&self, args: &[&str]) -> Result<String> {
        let output = run(self.command(args), None, self.deadline)?;
        succeeded(&args.join(" "), &output)
    }

    /// The one value git printed.
    fn line(&self, args: &[&str]) -> Result<String> {
        Ok(self.text(args)?.trim().to_string())
    }

    /// Adds every file to the index, but those in `skipped`. A file that cannot be read is
    /// passed over, as git does with `--ignore-errors`: the checkpoint holds the rest.
    fn add_all(&self, skipped: &[SkippedFile]) -> Result<()> {
        let mut args = vec!["add", "--all", "--ignore-errors"];
        let input = (!skipped.is_empty()).then(|| {
            args.extend(["--pathspec-from-file=-", "--pathspec-file-nul"]);
            excluding(skipped)
        });
        let output = run(self.command(&args), input, self.deadline)?;
        let said = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() && said.contains("fatal:") {
            return Err(Error::Git {
                args: args.join(" "),
                stderr: said.trim().to_string(),
            });
        }
        Ok(())
    }
}

/// The pathspecs for every file but those in `skipped`, each ended by a NUL.
fn excluding(skipped: &[SkippedFile]) -> Vec<u8> {
    let mut spec = b".\0".to_vec();
    for file in skipped {
        spec.extend_from_slice(format!(":(exclude,literal){}", file.path).as_bytes());
        spec.push(0);
    }
    spec
}

/// The untracked files too large to keep, with their sizes. A link is never followed: what
/// it points at is no part of the worktree.
fn too_large(git: &Git<'_>) -> Result<Vec<SkippedFile>> {
    let listed = git.text(&["ls-files", "--others", "--exclude-standard", "-z"])?;
    let size = |path: &str| {
        let found = std::fs::symlink_metadata(git.worktree.join(path)).ok()?;
        (found.is_file() && found.len() > MAX_UNTRACKED_BYTES).then_some(found.len())
    };
    Ok(nul_separated(&listed)
        .filter_map(|path| {
            size(path).map(|bytes| SkippedFile {
                path: path.to_string(),
                bytes,
            })
        })
        .collect())
}

/// [`SETTINGS`], with no hooks, and every filter the repository sets up switched off: a
/// filter such as Git LFS's writes into the repository when it cleans a file, and one that
/// is required would fail the checkpoint without it. What a checkpoint holds of such a file
/// is then the file as it is in the worktree.
fn settings(worktree: &Path, store: &Path, deadline: Instant) -> Result<Vec<(String, String)>> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(worktree)
        .args(["config", "--name-only", "--get-regexp", r"^filter\."])
        .env("GIT_TERMINAL_PROMPT", "0");
    // Nothing set up is an exit of 1 with nothing printed, which reads the same.
    let output = run(command, None, deadline)?;
    let named = String::from_utf8_lossy(&output.stdout).into_owned();
    let mut config: Vec<(String, String)> = SETTINGS
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect();
    let hooks = store.join("no-hooks").to_string_lossy().into_owned();
    config.push(("core.hooksPath".to_string(), hooks));
    for driver in filter_drivers(&named) {
        for (key, value) in [
            ("clean", ""),
            ("smudge", ""),
            ("process", ""),
            ("required", "false"),
        ] {
            config.push((format!("filter.{driver}.{key}"), value.to_string()));
        }
    }
    Ok(config)
}

/// The names of the filters `git config --name-only` lists keys of, each once.
fn filter_drivers(keys: &str) -> Vec<String> {
    let mut drivers: Vec<String> = keys
        .lines()
        .filter_map(|key| key.trim().strip_prefix("filter."))
        .filter_map(|rest| rest.rsplit_once('.').map(|(driver, _)| driver.to_string()))
        .collect();
    drivers.sort_unstable();
    drivers.dedup();
    drivers
}

/// `objects` as git reads one entry of `GIT_ALTERNATE_OBJECT_DIRECTORIES`, where a colon
/// separates entries: in quotes, the way git unquotes it, if it holds one.
fn alternate(objects: &Path) -> String {
    let path = objects.to_string_lossy();
    if !path.contains(':') && !path.starts_with('"') {
        return path.into_owned();
    }
    let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// What git printed, if it succeeded; otherwise what it said, as an error.
fn succeeded(what: &str, output: &Output) -> Result<String> {
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    Err(Error::Git {
        args: what.to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

/// Runs `command`, with `input` on its standard input, and waits for it until `deadline`, when
/// it is killed. Read and written on threads of their own, so that a full pipe holds nothing up.
fn run(mut command: Command, input: Option<Vec<u8>>, deadline: Instant) -> Result<Output> {
    let stdin = if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    command
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let writing = child.stdin.take().zip(input).map(|(mut pipe, input)| {
        std::thread::spawn(move || {
            pipe.write_all(&input).ok();
        })
    });
    let stdout = read_on(child.stdout.take());
    let stderr = read_on(child.stderr.take());
    let status = wait_until(&mut child, deadline);
    if let Some(writing) = writing {
        writing.join().ok();
    }
    Ok(Output {
        status: status?,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// Everything that comes out of `pipe`, read on a thread of its own.
fn read_on(pipe: Option<impl Read + Send + 'static>) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut read = Vec::new();
        if let Some(mut pipe) = pipe {
            pipe.read_to_end(&mut read).ok();
        }
        read
    })
}

/// How `child` ended, or an error once `deadline` has passed and it is killed.
fn wait_until(child: &mut Child, deadline: Instant) -> Result<ExitStatus> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            child.kill().ok();
            child.wait().ok();
            let late = std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "git took too long, and was stopped",
            );
            return Err(late.into());
        }
        std::thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_on_disk_where_a_file_would_come_back_is_in_the_way() {
        let kinds = "A\0.env\0M\0notes.txt\0D\0old.txt\0A\0src/new/a.rs\0";
        let nothing = |_: &str| None;
        assert_eq!(in_the_way(kinds, nothing), None);
        // A file there already, which the tree taken has not got.
        let env = |path: &str| (path == ".env").then_some(false);
        assert_eq!(in_the_way(kinds, env), Some(".env".to_string()));
        // A folder there is none: the file goes into it. A file where a folder has to be is.
        let folder = |path: &str| (path == "src").then_some(true);
        assert_eq!(in_the_way(kinds, folder), None);
        let file = |path: &str| (path == "src").then_some(false);
        assert_eq!(in_the_way(kinds, file), Some("src/new/a.rs".to_string()));
        // What is modified or deleted is in the tree taken, and so kept.
        let known = |path: &str| ["notes.txt", "old.txt"].contains(&path).then_some(false);
        assert_eq!(in_the_way(kinds, known), None);
    }

    #[test]
    fn a_file_that_gives_way_to_what_comes_back_is_not_in_the_way() {
        // A file that becomes a folder.
        let file = |path: &str| (path == "docs").then_some(false);
        assert_eq!(in_the_way("D\0docs\0A\0docs/a.md\0", file), None);
        // A name whose case changed, where both names find the one file.
        let there = |_: &str| Some(false);
        assert_eq!(in_the_way("D\0Readme.md\0A\0README.md\0", there), None);
    }

    #[test]
    fn a_copy_of_the_index_is_as_old_as_the_index() {
        let dir = tempfile::tempdir().expect("test setup");
        let (git_dir, store) = (dir.path().join("git"), dir.path().join("store"));
        std::fs::create_dir_all(&git_dir).expect("test setup");
        std::fs::create_dir_all(&store).expect("test setup");
        let index = git_dir.join("index");
        std::fs::write(&index, "an index").expect("test setup");
        let then = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let file = std::fs::File::options().write(true).open(&index);
        file.and_then(|file| file.set_modified(then))
            .expect("test setup");

        let scratch = Scratch::index(&store, &git_dir).expect("copies");
        let copied = std::fs::metadata(&scratch.0).and_then(|found| found.modified());
        assert_eq!(copied.expect("reads"), then);
        // No index yet is no copy, and no error.
        std::fs::remove_file(&index).expect("test setup");
        let none = Scratch::index(&store, &git_dir).expect("starts from none");
        assert!(!none.0.exists());
    }

    #[test]
    fn the_filters_set_up_are_named_once_each() {
        let keys = "filter.lfs.clean\nfilter.lfs.smudge\nfilter.lfs.required\n\
                    filter.my.odd.name.process\n";
        assert_eq!(filter_drivers(keys), ["lfs", "my.odd.name"]);
        assert_eq!(filter_drivers(""), Vec::<String>::new());
    }

    #[test]
    fn an_object_folder_with_a_colon_is_quoted_as_git_unquotes_it() {
        assert_eq!(alternate(Path::new("/r/.git/objects")), "/r/.git/objects");
        assert_eq!(
            alternate(Path::new("/my:repo/\"x\"/objects")),
            "\"/my:repo/\\\"x\\\"/objects\""
        );
    }

    #[test]
    fn the_files_too_large_are_left_out_by_their_literal_names() {
        let skipped = [SkippedFile {
            path: "data/*.bin".to_string(),
            bytes: 1,
        }];
        assert_eq!(excluding(&skipped), b".\0:(exclude,literal)data/*.bin\0");
    }
}

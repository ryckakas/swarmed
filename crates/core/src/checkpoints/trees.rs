//! Checkpoints as git trees: taken from a worktree into a store of objects of their own, and
//! compared, without writing anything into the repository.
//!
//! Git is pointed at the repository for everything it reads (what it ignores, its attributes,
//! the objects it has) and at the store for every object it writes, with an index of its own:
//! a copy of the worktree's, whose stat cache spares hashing what did not change. No ref, no
//! object and no index of the repository's is written.

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
    /// How long it took, in milliseconds.
    pub(crate) took_ms: u64,
}

/// The worktree's files as a tree in `store`: tracked and untracked files, as `git add -A`
/// sees them, but the untracked ones over [`MAX_UNTRACKED_BYTES`].
pub(crate) fn take(worktree: &Path, store: &Path) -> Result<Taken> {
    let started = Instant::now();
    let deadline = started + LIMIT;
    let repository = Repository::of(worktree, deadline)?;
    open_store(store, deadline)?;
    let index = Scratch::index(store, &repository.git_dir)?;
    let mut git = Git::new(worktree, store, &repository, deadline);
    git.config = settings(worktree, store, deadline)?;
    git.index = Some(index.0.clone());
    let skipped = too_large(&git)?;
    git.add_all(&skipped)?;
    let tree = git.line(&["write-tree"])?;
    let took = started.elapsed().as_millis();
    Ok(Taken {
        tree,
        skipped,
        took_ms: u64::try_from(took).unwrap_or(u64::MAX),
    })
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
        // A worktree with no index yet starts from none: every file is then hashed.
        match std::fs::copy(git_dir.join("index"), &path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error.into());
            }
            _ => {}
        }
        Ok(Self(path))
    }
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
                format!("git took longer than {} seconds", LIMIT.as_secs()),
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

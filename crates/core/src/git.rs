//! Git access by shelling out to the `git` binary.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tokio::process::Command;

use crate::error::{Error, Result};

/// Runs `git <args>` in `dir` and returns trimmed stdout.
pub(crate) async fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .await?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(Error::Git {
            args: args.join(" "),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

/// Where a worktree's `HEAD` is: on a branch, or on none (`None`, a detached `HEAD`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Head(pub(crate) Option<String>);

/// Where a worktree's `HEAD` is, asked of git without waiting on a runtime: for code that runs
/// after every tool call, where the answer is needed before anything else is done. `None`
/// when git cannot tell.
pub(crate) fn head_now(worktree: &Path) -> Option<Head> {
    // A folder that is no repository's top level, such as a chat's, is on no branch, even
    // inside a repository of the user's own (a home folder kept in git): git would answer for
    // that one.
    if !worktree.join(".git").exists() {
        return None;
    }
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(["branch", "--show-current"])
        .output()
        .ok()?;
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    output
        .status
        .success()
        .then(|| Head((!branch.is_empty()).then_some(branch)))
}

/// Why git would not take `name` as a branch's name, by its own rules
/// (`git check-ref-format --branch`), or `None` when it would. Asked without waiting on a
/// runtime: it needs no repository, and settings are saved from threads that have none.
///
/// Run inside a repository, git reads `@{-1}` and the like as the branch they stand for there
/// and prints that one. A name it prints back as another is no name: the branch made under
/// it would not be the one recorded.
pub(crate) fn branch_name_problem(name: &str) -> Result<Option<String>> {
    let output = std::process::Command::new("git")
        .args(["check-ref-format", "--branch", name])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if output.status.success() {
        let printed = String::from_utf8_lossy(&output.stdout);
        let another = printed.trim() != name;
        return Ok(another.then(|| format!("'{name}' is not a valid branch name")));
    }
    let said = String::from_utf8_lossy(&output.stderr);
    let said = said.trim();
    let reason = said.strip_prefix("fatal: ").unwrap_or(said);
    Ok(Some(reason.to_string()))
}

/// The branch a worktree is on, or none when it is on no branch (a detached `HEAD`).
pub(crate) async fn current_branch(worktree: &Path) -> Result<Option<String>> {
    let branch = git(worktree, &["branch", "--show-current"]).await?;
    Ok((!branch.is_empty()).then_some(branch))
}

/// The top-level directory of the repository containing `path`.
pub(crate) async fn repo_root(path: &Path) -> Result<PathBuf> {
    match git(path, &["rev-parse", "--show-toplevel"]).await {
        Ok(root) => Ok(root.into()),
        Err(Error::Git { .. } | Error::Io(_)) => Err(Error::NotARepo(path.to_path_buf())),
        Err(e) => Err(e),
    }
}

/// Whether the repository has any remote.
async fn has_remote(repo: &Path) -> Result<bool> {
    Ok(!git(repo, &["remote"]).await?.is_empty())
}

/// Fetches from the remote if there is one. A failed fetch (offline) is not an error:
/// the task then branches from whatever is known locally.
pub(crate) async fn fetch(repo: &Path) -> Result<bool> {
    if !has_remote(repo).await? {
        return Ok(false);
    }
    Ok(git(repo, &["fetch", "--quiet", "--prune"]).await.is_ok())
}

/// The ref new tasks branch from: `origin/<default>` when a remote exists, otherwise the
/// branch currently checked out in the main repo.
pub(crate) async fn default_base(repo: &Path) -> Result<String> {
    if let Ok(head) = git(
        repo,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .await
    {
        return Ok(head);
    }
    for candidate in ["origin/main", "origin/master"] {
        if ref_exists(repo, candidate).await {
            return Ok(candidate.to_string());
        }
    }
    git(repo, &["symbolic-ref", "--short", "HEAD"]).await
}

/// Gives a repository that has no commit yet a first, empty one on its current branch, so
/// that tasks have something in common to branch from. One that has commits is left alone.
///
/// The commit is made directly from an empty tree: whatever is in the working tree or staged
/// there stays exactly as it is, and is not swept into the commit.
pub(crate) async fn ensure_first_commit(repo: &Path) -> Result<()> {
    if ref_exists(repo, "HEAD").await {
        return Ok(());
    }
    let branch = git(repo, &["symbolic-ref", "HEAD"]).await?;
    let tree = git(repo, &["hash-object", "-t", "tree", "/dev/null"]).await?;
    let commit = git(repo, &["commit-tree", &tree, "-m", "Initial commit"]).await?;
    git(repo, &["update-ref", &branch, &commit]).await.map(drop)
}

/// Whether `name` resolves to a commit.
pub(crate) async fn ref_exists(repo: &Path, name: &str) -> bool {
    git(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{name}^{{commit}}"),
        ],
    )
    .await
    .is_ok()
}

/// Whether a local branch called `branch` exists.
pub(crate) async fn local_branch_exists(repo: &Path, branch: &str) -> bool {
    git(
        repo,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .await
    .is_ok()
}

/// The short names of every branch here and on any remote, as last fetched: `main`, and
/// `feature` for `origin/feature`. A new branch would collide with any of them, with a
/// remote one as soon as it is pushed.
pub(crate) async fn branch_names(repo: &Path) -> Result<HashSet<String>> {
    let local = git(
        repo,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .await?;
    let remote = git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname:lstrip=3)",
            "refs/remotes",
        ],
    )
    .await?;
    let names = local.lines().chain(remote.lines()).map(str::trim);
    Ok(names
        .filter(|name| !name.is_empty())
        .map(String::from)
        .collect())
}

/// Whether `name` is the local branch `branch`, however it is worded.
pub(crate) async fn names_branch(repo: &Path, name: &str, branch: &str) -> Result<bool> {
    let full = git(repo, &["rev-parse", "--symbolic-full-name", name]).await?;
    Ok(full == format!("refs/heads/{branch}"))
}

/// How many commits of local branch `branch` exist nowhere else: on neither `base` nor any
/// remote branch. None means deleting the branch loses nothing.
pub(crate) async fn commits_only_on(repo: &Path, branch: &str, base: &str) -> Result<u32> {
    let local = format!("refs/heads/{branch}");
    let count = git(
        repo,
        &["rev-list", "--count", &local, "--not", base, "--remotes"],
    )
    .await?;
    Ok(count.trim().parse().unwrap_or(0))
}

/// Creates a worktree at `path` on a new branch `branch` starting at `base`.
pub(crate) async fn worktree_add_new(
    repo: &Path,
    path: &Path,
    branch: &str,
    base: &str,
) -> Result<()> {
    let path = path.to_string_lossy();
    git(
        repo,
        &[
            "worktree",
            "add",
            "--quiet",
            "--no-track",
            "-b",
            branch,
            &path,
            base,
        ],
    )
    .await?;
    Ok(())
}

/// Creates a worktree at `path` for a branch that already exists locally or on the remote.
pub(crate) async fn worktree_add_existing(repo: &Path, path: &Path, branch: &str) -> Result<()> {
    let path = path.to_string_lossy();
    git(repo, &["worktree", "add", "--quiet", &path, branch]).await?;
    Ok(())
}

/// Removes a worktree. Always forced: `teardown` may leave ignored files behind (dependencies,
/// build output), and the caller has already checked for real work.
pub(crate) async fn worktree_remove(repo: &Path, path: &Path) -> Result<()> {
    git(
        repo,
        &["worktree", "remove", "--force", &path.to_string_lossy()],
    )
    .await?;
    Ok(())
}

/// Whether git still knows `path` as a worktree. Not when the repository's record of it is
/// gone, as a removal that failed halfway leaves it, nor when what git finds is a repository
/// in a folder above it.
pub(crate) async fn worktree_known(path: &Path) -> bool {
    let Ok(top) = git(path, &["rev-parse", "--show-toplevel"]).await else {
        return false;
    };
    let real = |path: &Path| std::fs::canonicalize(path).ok();
    real(path).is_some_and(|path| real(Path::new(top.trim())) == Some(path))
}

/// One worktree git keeps a record of, as `git worktree list` gives it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct WorktreeRecord {
    /// Where git has it.
    pub(crate) path: PathBuf,
    /// The local branch checked out in it; `None` when it is on none.
    pub(crate) branch: Option<String>,
    /// Whether its folder is gone, so that `git worktree prune` would forget it.
    pub(crate) prunable: bool,
    /// Whether it is locked, which keeps git from forgetting it even when its folder is gone.
    pub(crate) locked: bool,
}

/// Every worktree of the repository at `repo`, its own checkout first.
pub(crate) async fn worktree_list(repo: &Path) -> Result<Vec<WorktreeRecord>> {
    let listed = git(repo, &["worktree", "list", "--porcelain", "-z"]).await?;
    Ok(worktree_records(&listed))
}

/// What `git worktree list --porcelain -z` printed, as records: each begins with its path,
/// and every line of it is a field of its own, so that a path may hold anything.
fn worktree_records(listed: &str) -> Vec<WorktreeRecord> {
    let mut records: Vec<WorktreeRecord> = Vec::new();
    for field in crate::changes::nul_separated(listed) {
        if let Some(path) = field.strip_prefix("worktree ") {
            records.push(WorktreeRecord {
                path: PathBuf::from(path),
                ..WorktreeRecord::default()
            });
            continue;
        }
        let Some(record) = records.last_mut() else {
            continue;
        };
        let (name, _) = field.split_once(' ').unwrap_or((field, ""));
        match name {
            "branch" => {
                let branch = field.trim_start_matches("branch ");
                record.branch = branch.strip_prefix("refs/heads/").map(String::from);
            }
            "prunable" => record.prunable = true,
            "locked" => record.locked = true,
            _ => {}
        }
    }
    records
}

/// How many commits of the worktree's `HEAD` are on no branch and no remote: what a worktree
/// on no branch would take with it. None for one on a branch, which keeps its commits.
pub(crate) async fn commits_on_no_branch(worktree: &Path) -> Result<u32> {
    let count = git(
        worktree,
        &[
            "rev-list",
            "--count",
            "HEAD",
            "--not",
            "--branches",
            "--remotes",
        ],
    )
    .await?;
    Ok(count.trim().parse().unwrap_or(0))
}

/// Deletes a local branch, merged or not.
pub(crate) async fn branch_delete(repo: &Path, branch: &str) -> Result<()> {
    git(repo, &["branch", "-D", branch]).await?;
    Ok(())
}

/// Whether the worktree has modified, staged or untracked files.
pub(crate) async fn is_dirty(worktree: &Path) -> Result<bool> {
    Ok(!git(worktree, &["status", "--porcelain"]).await?.is_empty())
}

/// Commits on the worktree's branch that exist nowhere else: not on its upstream if it has
/// one, otherwise not on `base`.
pub(crate) async fn unpushed_commits(worktree: &Path, base: &str) -> Result<u32> {
    let range = match git(
        worktree,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
    .await
    {
        Ok(upstream) => format!("{upstream}..HEAD"),
        Err(_) => format!("{base}..HEAD"),
    };
    let count = git(worktree, &["rev-list", "--count", &range]).await?;
    Ok(count.parse().unwrap_or(0))
}

/// The branches a new worktree could check out: local branches and remote branches by their
/// short name, without the ones another worktree already has checked out. Sorted.
pub(crate) async fn available_branches(repo: &Path) -> Result<Vec<String>> {
    let local = git(
        repo,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .await?;
    // `lstrip=3` drops `refs/remotes/<remote>/`, which leaves the name `git worktree add`
    // resolves to a new tracking branch.
    let remote = git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname:lstrip=3)",
            "refs/remotes",
        ],
    )
    .await?;
    let worktrees = git(repo, &["worktree", "list", "--porcelain"]).await?;
    let taken: Vec<&str> = worktrees
        .lines()
        .filter_map(|line| line.strip_prefix("branch refs/heads/"))
        .collect();
    let mut branches: Vec<String> = local
        .lines()
        .chain(remote.lines())
        .filter(|name| !name.is_empty() && *name != "HEAD" && !taken.contains(name))
        .map(String::from)
        .collect();
    branches.sort_unstable();
    branches.dedup();
    Ok(branches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_records_are_read_field_by_field() {
        let listed = "worktree /r\0HEAD abc\0branch refs/heads/main\0\0\
                      worktree /w/a b\nc\0HEAD def\0detached\0\0\
                      worktree /w/gone\0HEAD 123\0branch refs/heads/feature/x\0\
                      prunable gitdir file points to non-existent location\0\0\
                      worktree /w/away\0HEAD 456\0branch refs/heads/b\0locked\0prunable\0\0";
        let records = worktree_records(listed);
        let path = |at: usize| records[at].path.to_string_lossy().into_owned();
        assert_eq!(records.len(), 4);
        assert_eq!(records[0].branch.as_deref(), Some("main"));
        // A path is taken whole, a line break in it included.
        assert_eq!(path(1), "/w/a b\nc");
        assert_eq!(records[1].branch, None);
        assert!(!records[1].prunable);
        assert_eq!(records[2].branch.as_deref(), Some("feature/x"));
        assert!(records[2].prunable && !records[2].locked);
        assert!(records[3].prunable && records[3].locked);
    }
}

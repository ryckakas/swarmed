//! Git access by shelling out to the `git` binary.

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

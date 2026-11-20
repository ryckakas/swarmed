//! Getting a task's work out of its worktree: committed, pushed, and into a pull request.
//!
//! Each step is something the user asked for by name; nothing here is done by itself. Commits
//! are made by the user's own git, as them, and the pull request is opened by their own `gh`.

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::agent_setup::find_program;
use crate::error::Error;
use crate::pulls::Pull;
use crate::{Core, Result, git};

/// What of a task's work has not left its worktree yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Unpublished {
    /// How many files are changed, added or removed and not committed.
    pub changed: u32,
    /// How many commits are not on the branch's upstream, or not on the task's base if it was
    /// never pushed.
    pub unpushed: u32,
    /// Whether the branch has been pushed before.
    pub pushed_before: bool,
}

/// A title and a description to start a pull request from, made of the task's commits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullDraft {
    /// The first commit's subject.
    pub title: String,
    /// One commit's own description, or the subjects of several as a list.
    pub body: String,
}

fn refused(why: impl Into<String>) -> Error {
    Error::Pull(why.into())
}

/// A draft from the task's commits, oldest first, each as its subject and its description.
fn draft_from(commits: &[(String, String)], task: &str) -> PullDraft {
    match commits {
        [] => PullDraft {
            title: task.replace('-', " "),
            body: String::new(),
        },
        [(subject, description)] => PullDraft {
            title: subject.clone(),
            body: description.clone(),
        },
        [(first, _), ..] => PullDraft {
            title: first.clone(),
            body: commits
                .iter()
                .map(|(subject, _)| format!("- {subject}"))
                .collect::<Vec<_>>()
                .join("\n"),
        },
    }
}

/// What `git log` printed with a unit separator between subject and description and a record
/// separator after each commit.
fn commits_in(log: &str) -> Vec<(String, String)> {
    log.split('\u{1e}')
        .filter_map(|record| record.trim().split_once('\u{1f}'))
        .map(|(subject, description)| (subject.trim().to_string(), description.trim().to_string()))
        .collect()
}

/// The branch a pull request goes into: the task's base without its remote's name.
fn base_branch(base_ref: &str) -> &str {
    base_ref.strip_prefix("origin/").unwrap_or(base_ref)
}

impl Core {
    /// What of a task's work is not committed, and what is not pushed.
    pub async fn task_unpublished(&self, project: &str, name: &str) -> Result<Unpublished> {
        let (_, task) = self.task(project, name)?;
        let worktree = &task.worktree_path;
        let status = git::git(worktree, &["status", "--porcelain"]).await?;
        let changed = status.lines().filter(|line| !line.is_empty()).count();
        let upstream = ["rev-parse", "--abbrev-ref", "@{upstream}"];
        Ok(Unpublished {
            changed: u32::try_from(changed).unwrap_or(u32::MAX),
            unpushed: git::unpushed_commits(worktree, &task.base_ref).await?,
            pushed_before: git::git(worktree, &upstream).await.is_ok(),
        })
    }

    /// Commits everything in a task's worktree, new files included, under `message`.
    pub async fn task_commit(&self, project: &str, name: &str, message: &str) -> Result<()> {
        let (_, task) = self.task(project, name)?;
        let message = message.trim();
        if message.is_empty() {
            return Err(refused("a commit needs a message"));
        }
        if !git::is_dirty(&task.worktree_path).await? {
            return Err(refused("there is nothing to commit"));
        }
        git::git(&task.worktree_path, &["add", "--all"]).await?;
        git::git(&task.worktree_path, &["commit", "--quiet", "-m", message]).await?;
        Ok(())
    }

    /// Pushes a task's branch to `origin`, and makes that its upstream.
    pub async fn task_push(&self, project: &str, name: &str) -> Result<()> {
        let (_, task) = self.task(project, name)?;
        let push = ["push", "--quiet", "--set-upstream", "origin", &task.branch];
        git::git(&task.worktree_path, &push).await?;
        Ok(())
    }

    /// A title and description to start the task's pull request from: its commits.
    pub async fn task_pull_draft(&self, project: &str, name: &str) -> Result<PullDraft> {
        let (_, task) = self.task(project, name)?;
        let range = format!("{}..HEAD", task.base_ref);
        let log = ["log", "--reverse", "--format=%s%x1f%b%x1e", &range];
        let log = git::git(&task.worktree_path, &log).await?;
        Ok(draft_from(&commits_in(&log), &task.name))
    }

    /// Opens a pull request for a task's branch into its base, with the user's own `gh`. The
    /// branch has to be pushed first.
    pub async fn task_pull_create(
        &self,
        project: &str,
        name: &str,
        draft: &PullDraft,
        as_draft: bool,
    ) -> Result<Pull> {
        let (_, task) = self.task(project, name)?;
        if draft.title.trim().is_empty() {
            return Err(refused("a pull request needs a title"));
        }
        let gh = find_program("gh", &self.tool_dirs).ok_or_else(|| {
            refused("the GitHub CLI `gh` is not installed: https://cli.github.com")
        })?;
        let base = base_branch(&task.base_ref);
        let mut command = Command::new(gh);
        command
            .args(["pr", "create", "--head", &task.branch, "--base", base])
            .args(["--title", draft.title.trim(), "--body", &draft.body])
            .args(as_draft.then_some("--draft"))
            .current_dir(&task.worktree_path)
            .stdin(std::process::Stdio::null());
        let output = command.output().await?;
        if !output.status.success() {
            let said = String::from_utf8_lossy(&output.stderr);
            return Err(refused(format!(
                "the pull request was not opened: {}",
                said.trim()
            )));
        }
        let pull = self.task_pull(project, name).await?;
        pull.ok_or_else(|| refused("the pull request was opened but cannot be read back yet"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(subject: &str, description: &str) -> (String, String) {
        (subject.to_string(), description.to_string())
    }

    #[test]
    fn one_commit_gives_its_subject_and_description() {
        let draft = draft_from(
            &[commit("fix: the loop", "It trusted `next`.")],
            "fix-login",
        );
        assert_eq!(draft.title, "fix: the loop");
        assert_eq!(draft.body, "It trusted `next`.");
    }

    #[test]
    fn several_commits_give_the_first_subject_and_a_list() {
        let commits = [commit("fix: the loop", "why"), commit("test: the loop", "")];
        let draft = draft_from(&commits, "fix-login");
        assert_eq!(draft.title, "fix: the loop");
        assert_eq!(draft.body, "- fix: the loop\n- test: the loop");
    }

    #[test]
    fn no_commits_give_the_tasks_name() {
        let draft = draft_from(&[], "fix-login-redirect");
        assert_eq!(draft.title, "fix login redirect");
        assert_eq!(draft.body, "");
    }

    #[test]
    fn the_log_is_split_into_commits() {
        let log = "one\u{1f}first body\nsecond line\n\u{1e}\ntwo\u{1f}\u{1e}\n";
        assert_eq!(
            commits_in(log),
            [commit("one", "first body\nsecond line"), commit("two", "")]
        );
        assert_eq!(commits_in(""), []);
    }

    #[test]
    fn a_pull_request_goes_into_the_base_without_its_remote() {
        assert_eq!(base_branch("origin/main"), "main");
        assert_eq!(base_branch("develop"), "develop");
    }
}

//! Getting a task's work out of its worktree: committed, pushed, and into a pull request.
//!
//! Each step is something the user asked for by name; nothing here is done by itself. Commits
//! are made by the user's own git, as them, and the pull request is opened by their own `gh`.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::agent::{COMMIT_INSTRUCTIONS, Effort, SessionSpec, Supervision};
use crate::agent_setup::find_program;
use crate::agents::CLAUDE;
use crate::error::Error;
use crate::pulls::Pull;
use crate::sessions::Session;
use crate::{Core, Result, asking, git};

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

/// How long an agent may take to write a commit message: it has to start first.
const ASKING_TIMEOUT: Duration = Duration::from_secs(90);

/// The most of a diff a commit message is written from, in characters. A message says what a
/// change is for, which its beginning and the list of files nearly always show.
const MAX_DIFF: usize = 24_000;

/// The largest new file a commit message is written from, in bytes, and how many of them.
const MAX_NEW_FILE: u64 = 8_000;
const MAX_NEW_FILES: usize = 20;

/// The text of the new files among `paths`, each under its name. `git diff` says nothing of
/// files git does not track yet, and a commit of everything takes them along. Large files
/// and ones that are not text are left to their names.
fn new_files(worktree: &Path, paths: &str) -> String {
    let small = |path: &&str| {
        std::fs::metadata(worktree.join(path))
            .is_ok_and(|file| file.is_file() && file.len() <= MAX_NEW_FILE)
    };
    let shown = |path: &str| {
        let text = std::fs::read_to_string(worktree.join(path)).ok()?;
        Some(format!("new file {path}:\n{text}\n"))
    };
    let paths = paths.lines().filter(small).take(MAX_NEW_FILES);
    paths.filter_map(shown).collect()
}

/// What a commit message is written from: the files that changed, then the diff, cut short
/// if it is long.
fn changes_for_message(status: &str, diff: &str) -> String {
    let shown: String = diff.chars().take(MAX_DIFF).collect();
    let cut = if shown.len() < diff.len() {
        "\n[The rest of the diff is left out.]"
    } else {
        ""
    };
    format!("git status --porcelain:\n{status}\n\ngit diff HEAD:\n{shown}{cut}")
}

/// A model's answer as a commit message: without the fence or quotes some put around it.
fn message_of(answer: &str) -> String {
    let lines = answer
        .trim()
        .lines()
        .filter(|line| !line.starts_with("```"));
    let text = lines.collect::<Vec<_>>().join("\n");
    text.trim().trim_matches(['"', '`']).trim().to_string()
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

    /// A commit message for everything in a task's worktree, for the user to read and change
    /// before anything is committed. It is written by the agent of `session`, on that
    /// session's model, in a conversation of its own; for a session on Claude Code, or without
    /// one, by the small model that titles sessions. Whoever writes is shown what git says
    /// changed, and nothing is committed here.
    pub async fn task_commit_message(
        &self,
        project: &str,
        name: &str,
        session: Option<i64>,
    ) -> Result<String> {
        let (_, task) = self.task(project, name)?;
        let worktree = &task.worktree_path;
        let status = git::git(worktree, &["status", "--porcelain"]).await?;
        if status.trim().is_empty() {
            return Err(refused("there is nothing to commit"));
        }
        let mut diff = git::git(worktree, &["diff", "HEAD"]).await?;
        let untracked = ["ls-files", "--others", "--exclude-standard"];
        diff.push('\n');
        diff.push_str(&new_files(worktree, &git::git(worktree, &untracked).await?));
        let changes = changes_for_message(&status, &diff);
        let writer = session.map(|id| self.session(id)).transpose()?;
        let answer = match writer.filter(|session| session.agent != CLAUDE) {
            Some(session) => self.ask_for_message(&session, changes).await?,
            None => self.name_message(changes).await?,
        };
        let message = answer.map(|answer| message_of(&answer));
        message
            .filter(|message| !message.is_empty())
            .ok_or_else(|| refused("no commit message came back: is the agent signed in?"))
    }

    /// Has the small model that titles sessions write the message.
    async fn name_message(&self, changes: String) -> Result<Option<String>> {
        let namer = self.namer.clone();
        let namer = namer.ok_or_else(|| refused("nothing here can write a commit message"))?;
        Ok(namer.commit_message(changes).await)
    }

    /// Has the agent of `session` write the message, on the session's model, in a
    /// conversation that has nothing to do with the session's own: in no worktree, with no
    /// tools allowed and none of Swarmed's servers.
    async fn ask_for_message(&self, session: &Session, changes: String) -> Result<Option<String>> {
        let backend = self.agents_now().backend(&session.agent)?;
        let spec = SessionSpec {
            cwd: std::env::temp_dir(),
            model: session.model.clone(),
            supervision: Supervision::Ask,
            effort: Some(Effort::Low),
            env: self.key_env()?,
            ..SessionSpec::default()
        };
        let question = format!("{COMMIT_INSTRUCTIONS}\n\n<changes>\n{changes}\n</changes>");
        Ok(asking::ask(backend, spec, question, ASKING_TIMEOUT).await)
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
    fn a_long_diff_is_cut_short_and_says_so() {
        let short = changes_for_message(" M a.rs", "diff --git a/a.rs");
        assert!(short.starts_with("git status --porcelain:\n M a.rs\n\ngit diff HEAD:\ndiff"));
        assert!(!short.contains("left out"));
        let long = changes_for_message("?? b", &"x".repeat(MAX_DIFF + 5));
        assert!(long.ends_with("[The rest of the diff is left out.]"));
        assert!(long.len() < MAX_DIFF + 200);
    }

    #[test]
    fn an_answer_becomes_a_message_without_its_wrapping() {
        assert_eq!(
            message_of("```\nfix: the login\n\nWhy.\n```\n"),
            "fix: the login\n\nWhy."
        );
        assert_eq!(message_of("\"feat: notes\"\n"), "feat: notes");
        assert_eq!(message_of("  \n"), "");
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

//! Getting a task's work out of its worktree: committed, pushed, and into a pull request.
//!
//! Each step is something the user asked for by name; nothing here is done by itself. Commits
//! are made by the user's own git, as them, and the pull request is opened by their own `gh`.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::agent::{COMMIT_INSTRUCTIONS, Effort, PULL_INSTRUCTIONS, SessionSpec, Supervision};
use crate::agent_setup::find_program;
use crate::agents::CLAUDE;
use crate::changes::untracked;
use crate::error::Error;
use crate::issues::{issue_for_message, with_closes};
use crate::pulls::Pull;
use crate::sessions::Session;
use crate::store::Task;
use crate::{Core, Result, asking, git};

/// What of a task's work has not left its worktree yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unpublished {
    /// How many files are changed, added or removed and not committed.
    pub changed: u32,
    /// How many commits are not on the branch's upstream, or not on the task's base if it was
    /// never pushed.
    pub unpushed: u32,
    /// Whether the branch has been pushed before.
    pub pushed_before: bool,
    /// The branch the worktree is on, which is the task's own unless something switched it;
    /// none when it is on no branch at all. The figures above are that branch's.
    pub branch: Option<String>,
    /// Every branch the worktree has been seen on that is still there, the task's own first.
    /// More than one means something switched it, and each has its own work and pull request.
    pub branches: Vec<String>,
    /// Whether a rebase is under way, which leaves the worktree on no branch until it is
    /// finished or aborted.
    pub rebasing: bool,
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
/// and ones that are not text are left to their names. So is a link: what it points at may
/// be anywhere on the machine, is not what would be committed, and is not the model's to read.
fn new_files(worktree: &Path, paths: &[String]) -> String {
    let small = |path: &&String| {
        std::fs::symlink_metadata(worktree.join(path))
            .is_ok_and(|file| file.is_file() && file.len() <= MAX_NEW_FILE)
    };
    let shown = |path: &String| {
        let text = std::fs::read_to_string(worktree.join(path)).ok()?;
        Some(format!("new file {path}:\n{text}\n"))
    };
    let paths = paths.iter().filter(small).take(MAX_NEW_FILES);
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

/// The most of a repository's pull request template that is handed on, in characters.
const MAX_TEMPLATE: usize = 4_000;

/// Where GitHub looks for a repository's pull request template.
const PULL_TEMPLATES: [&str; 3] = [
    ".github/pull_request_template.md",
    ".github/PULL_REQUEST_TEMPLATE.md",
    "docs/pull_request_template.md",
];

/// The repository's own template for pull requests, if it has one.
fn pull_template(worktree: &Path) -> Option<String> {
    let found = PULL_TEMPLATES
        .iter()
        .find_map(|path| std::fs::read_to_string(worktree.join(path)).ok())?;
    let template: String = found.chars().take(MAX_TEMPLATE).collect();
    (!template.trim().is_empty()).then_some(template)
}

/// What a pull request is written from: the branch's commits, the files it touches, its diff
/// cut short if it is long, and the repository's template if it has one.
fn branch_for_message(log: &str, stat: &str, diff: &str, template: Option<&str>) -> String {
    let shown: String = diff.chars().take(MAX_DIFF).collect();
    let cut = if shown.len() < diff.len() {
        "\n[The rest of the diff is left out.]"
    } else {
        ""
    };
    let template = template.map_or_else(String::new, |template| {
        format!("\n\nThe repository's pull request template:\n{template}")
    });
    format!(
        "The commits, oldest first:\n{log}\n\nFiles changed:\n{stat}\n\ngit diff:\n{shown}{cut}{template}"
    )
}

/// A model's answer as a pull request: its first line is the title, the rest the description.
fn draft_of(answer: &str) -> Option<PullDraft> {
    let message = message_of(answer);
    let (title, body) = message.split_once('\n').unwrap_or((&message, ""));
    let title = title.trim().trim_start_matches('#').trim();
    (!title.is_empty()).then(|| PullDraft {
        title: title.to_string(),
        body: body.trim().to_string(),
    })
}

/// What every agent is told about the task's branch and worktree, as part of Swarmed's
/// guidance. Swarmed follows a worktree switched to another branch, but the task's sessions,
/// its name and its pull request are about one piece of work: another belongs in a task of
/// its own, which the agent starts with `start_task` (`peers/tasks.rs`). Skills written for working without Swarmed often say to
/// branch out or make a worktree first, so this says plainly that it holds against them, and
/// how to read another branch without leaving this one.
pub(crate) fn worktree_instructions(branch: &str) -> String {
    format!(
        "\
# Your task's branch and worktree

You are already in an isolated git worktree that Swarmed made for this task, on the task's own \
branch, `{branch}`. Swarmed shows the user the task's changes, commits, pull request and checks \
from this worktree, so keep all of the work in it, on that branch. Committing and rebasing on \
this branch are fine.

Do not switch this worktree to another branch, do not create a branch to work on, and do not \
make another worktree or clone of the repository: no `git switch`, `git checkout <branch>`, \
`git switch -c`, `git checkout -b`, `git worktree add` or `git clone`, and no tool or option of \
your own that moves work into a worktree of its own (in Claude Code: `EnterWorktree`, or \
`isolation: \"worktree\"` for an agent). This holds even when a skill, a command or other \
instructions tell you to branch out or to work in a separate worktree or checkout: this \
worktree is that separate place already, so skip that step and follow the rest from here. \
For a part of the work that can go on beside you, start a session with Swarmed's \
`spawn_session` tool, if you have it, instead of a worktree: it works in this same worktree.

To look at another branch or a pull request, read it where it is: `git fetch` it, then \
`git show <ref>:<path>`, `git diff`, `git log` or `gh pr diff`.

When something needs a branch of its own (a separate change, a fix to another pull request, a \
fresh start from the base, the next pull request of a stack), it belongs in a new task in \
Swarmed, which gets its own branch, worktree and sessions. Start one with Swarmed's \
`start_task` tool: at once when the user asked for that work, and after asking them when it is \
only your own idea. Without that tool, ask the user to start the task. If the user asks you to \
switch or create a branch or a worktree here all the same, tell them that first, and do it if \
they still want it."
    )
}

/// Whether a rebase is under way in a worktree. `git am` keeps its own state in `rebase-apply`
/// too, but marks it `applying`, where a rebase marks it `rebasing`.
async fn rebase_under_way(worktree: &Path) -> Result<bool> {
    for state in ["rebase-merge", "rebase-apply/rebasing"] {
        // Relative to the worktree in a repository's own folder, whole in a linked worktree.
        let path = git::git(worktree, &["rev-parse", "--git-path", state]).await?;
        if worktree.join(path).exists() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Why nothing is done while a rebase is under way. Git switches no branch in the middle of
/// one, and the way out it suggests, `git rebase --quit`, leaves what was rebased on no branch.
fn mid_rebase() -> Error {
    refused("a rebase is under way in this worktree: finish it or abort it first")
}

/// The branch a task's worktree is on, which is the one a push or a pull request is of: the
/// work at hand is there, whichever branch the task began on. Refuses on no branch at all,
/// where there is nothing to name.
async fn branch_at_hand(task: &Task) -> Result<String> {
    if let Some(branch) = git::current_branch(&task.worktree_path).await? {
        return Ok(branch);
    }
    if rebase_under_way(&task.worktree_path).await? {
        return Err(mid_rebase());
    }
    Err(refused(
        "this worktree is on no branch: switch it to one first",
    ))
}

/// The branch a pull request goes into: the task's base without its remote's name.
fn base_branch(base_ref: &str) -> &str {
    base_ref.strip_prefix("origin/").unwrap_or(base_ref)
}

impl Core {
    /// What of a task's work is not committed, and what is not pushed, on the branch its
    /// worktree is on. A branch seen here for the first time is remembered for the task.
    pub async fn task_unpublished(&self, project: &str, name: &str) -> Result<Unpublished> {
        let (_, task) = self.repo_task(project, name)?;
        let worktree = &task.worktree_path;
        let status = git::git(worktree, &["status", "--porcelain"]).await?;
        let changed = status.lines().filter(|line| !line.is_empty()).count();
        let upstream = ["rev-parse", "--abbrev-ref", "@{upstream}"];
        let branch = git::current_branch(worktree).await?;
        // A rebase always leaves the worktree on no branch, so only then is it looked for.
        let rebasing = branch.is_none() && rebase_under_way(worktree).await?;
        Ok(Unpublished {
            changed: u32::try_from(changed).unwrap_or(u32::MAX),
            unpushed: git::unpushed_commits(worktree, &task.base_ref).await?,
            pushed_before: git::git(worktree, &upstream).await.is_ok(),
            branches: self.branches_of(&task, branch.as_deref()).await?,
            branch,
            rebasing,
        })
    }

    /// The branches of a task: its own, then the others its worktree was seen on, `current`
    /// among them from now on. One that has been deleted since is left out.
    async fn branches_of(&self, task: &Task, current: Option<&str>) -> Result<Vec<String>> {
        if let Some(current) = current.filter(|current| *current != task.branch) {
            self.store().task_branch_seen(task.id, current)?;
        }
        let seen = self.store().task_branches(task.id)?;
        let mut branches = vec![task.branch.clone()];
        for branch in seen {
            let there = git::local_branch_exists(&task.worktree_path, &branch).await;
            branches.extend((there && !branches.contains(&branch)).then_some(branch));
        }
        Ok(branches)
    }

    /// Puts a task's worktree on `branch`, one of those it has been on: the task's own or one
    /// something switched it to. Git takes changes that are not committed along when they do
    /// not conflict, and refuses with its reason when they do; every branch keeps its commits.
    /// Refused while a rebase is under way.
    pub async fn task_switch(&self, project: &str, name: &str, branch: &str) -> Result<()> {
        let (project, task) = self.repo_task(project, name)?;
        let worktree = &task.worktree_path;
        if rebase_under_way(worktree).await? {
            return Err(mid_rebase());
        }
        let current = git::current_branch(worktree).await?;
        let known = self.branches_of(&task, current.as_deref()).await?;
        if !known.iter().any(|known| known == branch) {
            return Err(refused(format!(
                "`{branch}` is not a branch this task's worktree has been on"
            )));
        }
        git::switch(project.repo()?, worktree, branch).await?;
        // The user moved it: nothing for the watch for moves to report, and each agent of the
        // task is told with its next message.
        self.live
            .branch_seen(worktree, &git::Head(Some(branch.to_string())));
        Ok(())
    }

    /// Commits everything in a task's worktree, new files included, under `message`.
    pub async fn task_commit(&self, project: &str, name: &str, message: &str) -> Result<()> {
        let (_, task) = self.repo_task(project, name)?;
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
        let (_, task) = self.repo_task(project, name)?;
        let worktree = &task.worktree_path;
        let status = git::git(worktree, &["status", "--porcelain"]).await?;
        if status.trim().is_empty() {
            return Err(refused("there is nothing to commit"));
        }
        let mut diff = git::git(worktree, &["diff", "HEAD"]).await?;
        diff.push('\n');
        diff.push_str(&new_files(worktree, &untracked(worktree).await?));
        let changes = changes_for_message(&status, &diff);
        let writer = session.map(|id| self.session(id)).transpose()?;
        let answer = match writer.filter(|session| session.agent != CLAUDE) {
            Some(session) => {
                self.ask_for_message(&session, COMMIT_INSTRUCTIONS, &changes)
                    .await?
            }
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
    async fn ask_for_message(
        &self,
        session: &Session,
        instructions: &str,
        changes: &str,
    ) -> Result<Option<String>> {
        let backend = self.agents_now().backend(&session.agent)?;
        let spec = SessionSpec {
            cwd: std::env::temp_dir(),
            model: session.model.clone(),
            supervision: Supervision::Ask,
            effort: Some(Effort::Low),
            env: self.key_env()?,
            ..SessionSpec::default()
        };
        let question = format!("{instructions}\n\n<changes>\n{changes}\n</changes>");
        Ok(asking::ask(backend, spec, question, ASKING_TIMEOUT).await)
    }

    /// Pushes the branch a task's worktree is on to `origin`, and makes that its upstream:
    /// the task's own, unless something switched the worktree to another.
    pub async fn task_push(&self, project: &str, name: &str) -> Result<()> {
        let (_, task) = self.repo_task(project, name)?;
        let branch = branch_at_hand(&task).await?;
        let push = ["push", "--quiet", "--set-upstream", "origin", &branch];
        git::git(&task.worktree_path, &push).await?;
        Ok(())
    }

    /// A title and description to start the task's pull request from: its commits. For a
    /// task started for an issue, the description closes it (`Closes #12`).
    pub async fn task_pull_draft(&self, project: &str, name: &str) -> Result<PullDraft> {
        let (_, task) = self.repo_task(project, name)?;
        let range = format!("{}..HEAD", task.base_ref);
        let log = ["log", "--reverse", "--format=%s%x1f%b%x1e", &range];
        let log = git::git(&task.worktree_path, &log).await?;
        let draft = draft_from(&commits_in(&log), &task.name);
        Ok(PullDraft {
            body: with_closes(&draft.body, task.issue),
            ..draft
        })
    }

    /// A title and a description for the task's pull request, written from its commits and
    /// from what the branch changes against its base, for the user to read and change before
    /// anything is opened. Written by the agent of `session` on that session's model, in a
    /// conversation of its own; for a session on Claude Code, or without one, by the small
    /// model that titles sessions. A repository's own pull request template is filled in. For
    /// a task started for an issue, the writer is told so, and the description closes it.
    pub async fn task_pull_message(
        &self,
        project: &str,
        name: &str,
        session: Option<i64>,
    ) -> Result<PullDraft> {
        let (_, task) = self.repo_task(project, name)?;
        let worktree = &task.worktree_path;
        let range = format!("{}..HEAD", task.base_ref);
        let log = ["log", "--reverse", "--format=%s%n%b", &range];
        let log = git::git(worktree, &log).await?;
        if log.trim().is_empty() {
            return Err(refused(
                "there is nothing to describe yet: the branch has no commits of its own",
            ));
        }
        // Three dots: what the branch itself changed, not what its base has moved on by.
        let against = format!("{}...HEAD", task.base_ref);
        let stat = git::git(worktree, &["diff", "--stat", &against]).await?;
        let diff = git::git(worktree, &["diff", &against]).await?;
        let template = pull_template(worktree);
        let mut branch = branch_for_message(&log, &stat, &diff, template.as_deref());
        branch.push_str(&issue_for_message(task.issue));
        let writer = session.map(|id| self.session(id)).transpose()?;
        let answer = if let Some(session) = writer.filter(|session| session.agent != CLAUDE) {
            self.ask_for_message(&session, PULL_INSTRUCTIONS, &branch)
                .await?
        } else {
            let namer = self.namer.clone();
            let namer = namer.ok_or_else(|| refused("nothing here can write a pull request"))?;
            namer.pull_message(branch).await
        };
        let draft = answer.as_deref().and_then(draft_of);
        let draft = draft
            .ok_or_else(|| refused("no pull request text came back: is the agent signed in?"))?;
        // Kept even where the writer left it out: the task's issue closes with its merge.
        Ok(PullDraft {
            body: with_closes(&draft.body, task.issue),
            ..draft
        })
    }

    /// Opens a pull request for the branch a task's worktree is on into the task's base, with
    /// the user's own `gh`. The branch has to be pushed first.
    pub async fn task_pull_create(
        &self,
        project: &str,
        name: &str,
        draft: &PullDraft,
        as_draft: bool,
    ) -> Result<Pull> {
        let (_, task) = self.repo_task(project, name)?;
        if draft.title.trim().is_empty() {
            return Err(refused("a pull request needs a title"));
        }
        let branch = branch_at_hand(&task).await?;
        let gh = find_program("gh", &self.tool_dirs).ok_or_else(|| {
            refused("the GitHub CLI `gh` is not installed: https://cli.github.com")
        })?;
        let base = base_branch(&task.base_ref);
        let mut command = Command::new(gh);
        command
            .args(["pr", "create", "--head", &branch, "--base", base])
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

    #[cfg(unix)]
    #[test]
    fn a_new_link_is_named_and_what_it_points_at_is_not_shown() {
        let outside = tempfile::tempdir().expect("temp dir");
        let secret = outside.path().join("credentials");
        std::fs::write(&secret, "key = hunter2\n").expect("writes");
        let worktree = tempfile::tempdir().expect("temp dir");
        std::os::unix::fs::symlink(&secret, worktree.path().join("creds")).expect("links");
        std::fs::write(worktree.path().join("r\u{e9}sum\u{e9}.txt"), "hello\n").expect("writes");

        let paths = ["creds".to_string(), "r\u{e9}sum\u{e9}.txt".to_string()];
        let shown = new_files(worktree.path(), &paths);
        assert_eq!(shown, "new file r\u{e9}sum\u{e9}.txt:\nhello\n\n");
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

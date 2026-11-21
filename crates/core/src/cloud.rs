//! Handing work to an agent that runs in its provider's cloud instead of on this machine.
//!
//! That is Codex Cloud or a Claude Code cloud session, each started through the user's own
//! `codex` or `claude` program and the login it holds. A delegated task runs on the provider's
//! copy of the repository, taken from GitHub: it sees what was pushed, not what is only in a
//! worktree here. Swarmed keeps the task's link, and its status where the provider's program
//! can be asked for one. What the task produces is read at the provider.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Row, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::process::Command;

use crate::agent_setup::find_program;
use crate::error::{Error, Result};
use crate::store::Project;
use crate::{Core, git, open};

/// How many cloud tasks one session may have delegated. An agent hands off a part of its
/// work, not all of it many times over.
pub const MAX_DELEGATED: usize = 6;

/// How long Claude Code may take to create a cloud session. It prints the link within
/// seconds; if it is still there after this, it is waiting for an answer nobody can give.
const CLAUDE_TIMEOUT: Duration = Duration::from_secs(90);

/// Whose cloud a task is handed to.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudProvider {
    /// Codex Cloud, in the environment the user has set up for the repository.
    #[default]
    Codex,
    /// A Claude Code cloud session, in the user's default cloud environment.
    Claude,
}

impl CloudProvider {
    /// The name it is stored and shown under.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }

    /// The provider called `name`, if there is one.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        [Self::Codex, Self::Claude]
            .into_iter()
            .find(|provider| provider.as_str() == name)
    }
}

/// A task as its provider started it.
struct Started {
    remote_id: String,
    url: String,
    branch: String,
    /// What is known of it at once: `pending` where its status can be asked for later, `sent`
    /// where it cannot.
    status: &'static str,
}

/// A task handed to a cloud agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CloudTask {
    /// Swarmed's own id for it.
    pub id: i64,
    /// The project it was delegated from.
    pub project_id: i64,
    /// Whose cloud it runs in: `codex` or `claude`.
    pub provider: String,
    /// The provider's id for it.
    pub remote_id: String,
    /// Where it is looked at in a browser.
    pub url: String,
    /// What it was asked to do.
    pub prompt: String,
    /// The branch it started from.
    pub branch: String,
    /// What the provider last said of it, such as `pending`, `ready` or `error`.
    pub status: String,
    /// The session that delegated it, if an agent did.
    pub session_id: Option<i64>,
}

/// Who delegates, which decides the branch the cloud agent starts from.
#[derive(Debug, Default, Clone, Copy)]
pub struct CloudOrigin<'a> {
    /// The task whose branch to start from, if it is on GitHub; else the project's base.
    pub task: Option<&'a str>,
    /// The session asking, if an agent is.
    pub session: Option<i64>,
    /// Whose cloud to hand the work to.
    pub provider: CloudProvider,
}

const SELECT: &str = "SELECT id, project_id, provider, remote_id, url, prompt, branch, status, \
                      session_id FROM cloud_tasks";

fn from_row(row: &Row<'_>) -> rusqlite::Result<CloudTask> {
    Ok(CloudTask {
        id: row.get(0)?,
        project_id: row.get(1)?,
        provider: row.get(2)?,
        remote_id: row.get(3)?,
        url: row.get(4)?,
        prompt: row.get(5)?,
        branch: row.get(6)?,
        status: row.get(7)?,
        session_id: row.get(8)?,
    })
}

fn refused(reason: impl Into<String>) -> Error {
    Error::Cloud(reason.into())
}

/// The `owner/repo` a GitHub remote address names, or `None` for any other address.
fn github_repo(remote: &str) -> Option<String> {
    let address = remote.trim().trim_end_matches('/');
    let path = [
        "git@github.com:",
        "ssh://git@github.com/",
        "https://github.com/",
    ]
    .iter()
    .find_map(|prefix| address.strip_prefix(prefix))?;
    let path = path.trim_end_matches(".git");
    let (owner, repo) = path.split_once('/')?;
    let plain = !owner.is_empty() && !repo.is_empty() && !repo.contains('/');
    plain.then(|| format!("{owner}/{repo}"))
}

/// The provider's id in the address of a task: its last piece.
fn remote_id(url: &str) -> Option<&str> {
    let id = url.trim().rsplit('/').next()?;
    (url.trim().starts_with("https://") && !id.is_empty()).then_some(id)
}

/// The statuses in what `codex cloud list --json` prints, by task id.
fn statuses(listing: &str) -> Vec<(String, String)> {
    let parsed: Value = serde_json::from_str(listing).unwrap_or_default();
    let tasks = parsed.get("tasks").and_then(Value::as_array);
    let status = |task: &Value| {
        let text = |key: &str| task.get(key)?.as_str().map(String::from);
        text("id").zip(text("status"))
    };
    tasks.map_or_else(Vec::new, |tasks| tasks.iter().filter_map(status).collect())
}

/// What one run of `codex cloud` came to.
struct Ran {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl Core {
    /// The cloud tasks delegated from `project`, newest first. With `refresh`, their statuses
    /// are asked for first; if that fails, the last known ones are given.
    pub async fn cloud_tasks(&self, project: &str, refresh: bool) -> Result<Vec<CloudTask>> {
        let project = self.project(project)?;
        if refresh && self.has_cloud_tasks(project.id)? {
            self.refresh_cloud(project.id, &project.repo_path)
                .await
                .ok();
        }
        self.cloud_tasks_of(Some(project.id))
    }

    /// Every cloud task, or those of one project, newest first.
    pub(crate) fn cloud_tasks_of(&self, project: Option<i64>) -> Result<Vec<CloudTask>> {
        let store = self.store();
        let sql = format!("{SELECT} WHERE ?1 IS NULL OR project_id = ?1 ORDER BY id DESC");
        let mut statement = store.conn.prepare(&sql)?;
        let rows = statement.query_map(params![project], from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn has_cloud_tasks(&self, project: i64) -> Result<bool> {
        Ok(!self.cloud_tasks_of(Some(project))?.is_empty())
    }

    /// Opens a cloud task's page at its provider in the default browser.
    pub fn cloud_task_open(&self, id: i64) -> Result<()> {
        let tasks = self.cloud_tasks_of(None)?;
        let task = tasks.iter().find(|task| task.id == id);
        let task = task.ok_or_else(|| refused(format!("there is no cloud task {id}")))?;
        open::in_browser(&task.url)
    }

    /// Takes a cloud task off Swarmed's list. It is not stopped: that is done at the provider.
    pub fn cloud_task_forget(&self, id: i64) -> Result<()> {
        let store = self.store();
        store
            .conn
            .execute("DELETE FROM cloud_tasks WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Hands `prompt` to a cloud agent working on `project`'s repository.
    ///
    /// Refused, with what to do about it, if the repository is not on GitHub or has no cloud
    /// environment at the provider, and for a session that has delegated its share already.
    pub async fn cloud_delegate(
        &self,
        project: &str,
        prompt: &str,
        origin: CloudOrigin<'_>,
    ) -> Result<CloudTask> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err(Error::EmptyMessage);
        }
        let project = self.project(project)?;
        self.within_share(origin.session)?;
        let repo = self.cloud_repo(&project.repo_path).await?;
        let started = match origin.provider {
            CloudProvider::Codex => self.start_codex(&project, &repo, prompt, origin).await?,
            CloudProvider::Claude => self.start_claude(&project, prompt).await?,
        };
        let store = self.store();
        store.conn.execute(
            "INSERT INTO cloud_tasks (project_id, provider, remote_id, url, prompt, branch, \
             status, session_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                project.id,
                origin.provider.as_str(),
                started.remote_id,
                started.url,
                prompt,
                started.branch,
                started.status,
                origin.session
            ],
        )?;
        let sql = format!("{SELECT} WHERE id = ?1");
        let new = store.conn.last_insert_rowid();
        Ok(store.conn.query_row(&sql, params![new], from_row)?)
    }

    /// Starts a Codex Cloud task in the environment of `repo`, which there has to be.
    async fn start_codex(
        &self,
        project: &Project,
        repo: &str,
        prompt: &str,
        origin: CloudOrigin<'_>,
    ) -> Result<Started> {
        self.require_cloud_environment(repo).await?;
        let branch = self
            .cloud_branch(&project.name, &project.repo_path, origin.task)
            .await?;
        // After `--`, so that a prompt that starts with a dash is not read as an option.
        let exec = [
            "cloud", "exec", "--env", repo, "--branch", &branch, "--", prompt,
        ];
        let ran = self.codex(&exec).await?;
        let url = ran.stdout.lines().find(|line| remote_id(line).is_some());
        let Some((url, id)) = url.and_then(|url| Some((url.trim(), remote_id(url)?))) else {
            return Err(refused(format!(
                "Codex did not start the task: {}",
                last_line(&ran.stderr)
            )));
        };
        Ok(Started {
            remote_id: id.to_string(),
            url: url.to_string(),
            branch,
            status: "pending",
        })
    }

    /// Starts a Claude Code cloud session on the repository, as checked out in its own folder.
    ///
    /// Claude Code only creates one with a terminal attached, so it is given one. It is run in
    /// the repository's folder and not in a task's worktree: it asks once per folder whether
    /// the folder is trusted, a question only the user may answer, and a worktree is a new
    /// folder every time.
    async fn start_claude(&self, project: &Project, prompt: &str) -> Result<Started> {
        let program = find_program("claude", &self.tool_dirs).ok_or_else(|| {
            refused("Claude Code is not installed: install it from Settings, under Agents")
        })?;
        let repo = &project.repo_path;
        let mut command = with_terminal(&program, &["--cloud", prompt]);
        command
            .current_dir(repo)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        // As for a session here: the user's own login, whatever Swarmed was started from.
        let inherited = std::env::vars_os().map(|(name, _)| name);
        for name in inherited.filter(|name| {
            let name = name.to_string_lossy();
            name.starts_with("ANTHROPIC_") || name.starts_with("CLAUDE")
        }) {
            command.env_remove(name);
        }
        let Ok(output) = tokio::time::timeout(CLAUDE_TIMEOUT, command.output()).await else {
            return Err(refused(format!(
                "Claude Code did not answer. It is probably asking whether to trust {}: run \
                 `claude` in that folder once, accept, then delegate again",
                repo.display()
            )));
        };
        let said = String::from_utf8_lossy(&output?.stdout).into_owned();
        let Some((url, id)) = claude_session(&said) else {
            return Err(refused(format!(
                "Claude Code did not start a cloud session: {}",
                last_line(&plain(&said))
            )));
        };
        let branch = git::git(repo, &["symbolic-ref", "--short", "HEAD"]).await;
        Ok(Started {
            remote_id: id,
            url,
            branch: branch.unwrap_or_default(),
            status: "sent",
        })
    }

    /// Refuses a session that has as many cloud tasks to its name as one may have.
    fn within_share(&self, session: Option<i64>) -> Result<()> {
        let Some(session) = session else {
            return Ok(());
        };
        let delegated = self.cloud_tasks_of(None)?;
        let own = delegated
            .iter()
            .filter(|task| task.session_id == Some(session));
        if own.count() >= MAX_DELEGATED {
            return Err(refused(format!(
                "this session has delegated {MAX_DELEGATED} cloud tasks, the most allowed"
            )));
        }
        Ok(())
    }

    /// The repository's name on GitHub, which is where a cloud agent takes it from.
    async fn cloud_repo(&self, repo: &Path) -> Result<String> {
        let remote = git::git(repo, &["remote", "get-url", "origin"]).await.ok();
        remote.as_deref().and_then(github_repo).ok_or_else(|| {
            refused(
                "this repository is not on GitHub, and a cloud agent can only work on one that \
                 is: push it to GitHub first",
            )
        })
    }

    /// Fails, saying what to do, unless Codex has a cloud environment for `repo`.
    async fn require_cloud_environment(&self, repo: &str) -> Result<()> {
        let list = ["cloud", "list", "--env", repo, "--json", "--limit", "1"];
        let ran = self.codex(&list).await?;
        if ran.ok {
            return Ok(());
        }
        if ran.stderr.contains("not found") {
            return Err(refused(format!(
                "{repo} has no Codex cloud environment. Create one for it at \
                 https://chatgpt.com/codex, under Environments, then delegate again"
            )));
        }
        Err(refused(format!(
            "Codex could not be asked about {repo}: {}",
            last_line(&ran.stderr)
        )))
    }

    /// The branch a cloud agent starts from: the task's own if GitHub has it, else the
    /// project's base branch.
    async fn cloud_branch(&self, project: &str, repo: &Path, task: Option<&str>) -> Result<String> {
        if let Some(pushed) = self.pushed_branch(project, repo, task).await? {
            return Ok(pushed);
        }
        let config = self.project_config(project)?;
        let base = if config.base_branch.is_empty() {
            git::default_base(repo).await?
        } else {
            config.base_branch
        };
        Ok(base.trim_start_matches("origin/").to_string())
    }

    /// The branch of `task`, if there is such a task and GitHub has its branch.
    async fn pushed_branch(
        &self,
        project: &str,
        repo: &Path,
        task: Option<&str>,
    ) -> Result<Option<String>> {
        let Some(task) = task else {
            return Ok(None);
        };
        let (_, task) = self.task(project, task)?;
        let on_github = format!("refs/remotes/origin/{}", task.branch);
        Ok(git::ref_exists(repo, &on_github)
            .await
            .then_some(task.branch))
    }

    /// Asks Codex for the statuses of the project's cloud tasks and keeps them. Claude Code
    /// has no way of being asked, so its sessions stay as they were started.
    async fn refresh_cloud(&self, project: i64, repo: &Path) -> Result<()> {
        let repo = self.cloud_repo(repo).await?;
        let list = ["cloud", "list", "--env", &repo, "--json", "--limit", "20"];
        let ran = self.codex(&list).await?;
        let store = self.store();
        for (id, status) in statuses(&ran.stdout) {
            store.conn.execute(
                "UPDATE cloud_tasks SET status = ?1 WHERE project_id = ?2 AND remote_id = ?3 \
                 AND provider = 'codex'",
                params![status, project, id],
            )?;
        }
        Ok(())
    }

    /// The Codex cloud task `id`, or why there is nothing of the kind to ask Codex about.
    fn codex_cloud_task(&self, id: i64) -> Result<CloudTask> {
        let tasks = self.cloud_tasks_of(None)?;
        let task = tasks.into_iter().find(|task| task.id == id);
        let task = task.ok_or_else(|| refused(format!("there is no cloud task {id}")))?;
        if task.provider != CloudProvider::Codex.as_str() {
            return Err(refused(
                "only a Codex cloud task's changes can be read here: a Claude cloud session is \
                 brought to this machine with `claude --teleport <id>` in a terminal",
            ));
        }
        Ok(task)
    }

    /// What a Codex cloud task changed, as a unified diff. A task that is still running, or
    /// changed nothing, has none, which Codex says in its own words.
    pub async fn cloud_task_diff(&self, id: i64) -> Result<String> {
        let task = self.codex_cloud_task(id)?;
        let ran = self.codex(&["cloud", "diff", &task.remote_id]).await?;
        if ran.ok {
            Ok(ran.stdout)
        } else {
            Err(refused(said(&ran)))
        }
    }

    /// Applies what a Codex cloud task changed to a local task's worktree, uncommitted, for
    /// the user to look at and commit there.
    pub async fn cloud_task_apply(&self, id: i64, project: &str, name: &str) -> Result<()> {
        let cloud = self.codex_cloud_task(id)?;
        let (_, task) = self.task(project, name)?;
        let args = ["cloud", "apply", &cloud.remote_id];
        let ran = self.codex_in(&task.worktree_path, &args).await?;
        if ran.ok {
            Ok(())
        } else {
            Err(refused(said(&ran)))
        }
    }

    /// Runs the user's `codex` with `args`, in a folder of Swarmed's own: Codex leaves a log
    /// where it is run, and that must not land in a repository.
    async fn codex(&self, args: &[&str]) -> Result<Ran> {
        let folder: PathBuf = self.paths.data_dir.join("run").join("cloud");
        tokio::fs::create_dir_all(&folder).await?;
        self.codex_in(&folder, args).await
    }

    /// Runs the user's `codex` with `args` in `folder`, and takes away the log it leaves
    /// there, unless a file of that name was the user's own to begin with.
    async fn codex_in(&self, folder: &Path, args: &[&str]) -> Result<Ran> {
        let program = find_program("codex", &self.tool_dirs).ok_or_else(|| {
            refused("Codex is not installed: install it from Settings, under Agents")
        })?;
        let log = folder.join("error.log");
        let theirs = log.exists();
        let output = Command::new(program)
            .args(args)
            .current_dir(folder)
            .stdin(std::process::Stdio::null())
            .output()
            .await?;
        // It names the user's account, and nobody reads it.
        if !theirs {
            tokio::fs::remove_file(log).await.ok();
        }
        Ok(Ran {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// What Codex said went wrong, without the word it starts every such line with.
fn said(ran: &Ran) -> String {
    let text = if ran.stderr.trim().is_empty() {
        &ran.stdout
    } else {
        &ran.stderr
    };
    let text = text.trim();
    text.strip_prefix("Error: ").unwrap_or(text).to_string()
}

/// `program` with `args`, run with a terminal of its own where this system has the means.
/// Elsewhere it is run as it is, which is enough for a stand-in.
fn with_terminal(program: &Path, args: &[&str]) -> Command {
    if cfg!(target_os = "macos") {
        let mut command = Command::new("script");
        command.args(["-q", "/dev/null"]).arg(program).args(args);
        return command;
    }
    let mut command = Command::new(program);
    command.args(args);
    command
}

/// `text` without the control sequences a terminal program writes around its words.
fn plain(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut rest = text.chars();
    while let Some(c) = rest.next() {
        if c == '\u{1b}' {
            // An escape, then everything up to the letter or `~` that ends the sequence.
            rest.by_ref().find(|c| c.is_ascii_alphabetic() || *c == '~');
        } else if c == '\n' || !c.is_control() {
            plain.push(c);
        }
    }
    plain
}

/// The address and the id of the cloud session Claude Code says it created, from the line
/// `View: https://claude.ai/code/session_…`. The address is kept without what follows `?`.
fn claude_session(said: &str) -> Option<(String, String)> {
    let said = plain(said);
    let start = said.find("https://claude.ai/code/")?;
    let address = said[start..]
        .split(|c: char| c.is_whitespace() || c == '?')
        .next()?;
    let id = address.rsplit('/').next()?;
    let known = id.starts_with("session_") || id.starts_with("cse_");
    known.then(|| (address.to_string(), id.to_string()))
}

/// The last line of what a program said went wrong, which is where it says it.
fn last_line(text: &str) -> &str {
    let said = text.lines().rev().find(|line| !line.trim().is_empty());
    said.map_or("it gave no reason", str::trim)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_github_remote_names_its_repository_and_nothing_else_does() {
        let named = Some("acme/tool".to_string());
        assert_eq!(github_repo("https://github.com/acme/tool.git\n"), named);
        assert_eq!(github_repo("https://github.com/acme/tool"), named);
        assert_eq!(github_repo("git@github.com:acme/tool.git"), named);
        assert_eq!(github_repo("ssh://git@github.com/acme/tool"), named);
        assert_eq!(github_repo("https://gitlab.com/acme/tool.git"), None);
        assert_eq!(github_repo("/Users/me/code/tool"), None);
        assert_eq!(github_repo("https://github.com/acme"), None);
    }

    #[test]
    fn a_task_is_known_by_the_last_piece_of_its_address() {
        let url = "https://chatgpt.com/codex/tasks/task_b_123\n";
        assert_eq!(remote_id(url), Some("task_b_123"));
        assert_eq!(remote_id("Error: environment not found"), None);
        assert_eq!(remote_id(""), None);
    }

    #[test]
    fn the_session_claude_code_created_is_read_off_its_terminal_output() {
        let said = "\u{4}\u{8}\u{8}\u{1b}7\u{1b}8Created cloud session: Connectivity test\r\n\
                    View: https://claude.ai/code/session_01TFabc?from=cli&m=0\r\n\
                    Resume with: claude --teleport session_01TFabc\r\n\u{1b}[>4m\u{1b}[<u";
        let session = claude_session(said).expect("found");
        assert_eq!(session.0, "https://claude.ai/code/session_01TFabc");
        assert_eq!(session.1, "session_01TFabc");
        // Asked whether the folder is trusted, it has created nothing.
        let asking = "\u{1b}[1mAccessing workspace:\u{1b}[0m\r\n Yes, I trust this folder";
        assert_eq!(claude_session(asking), None);
        assert_eq!(claude_session("see https://claude.ai/code/ for more"), None);
        assert_eq!(
            plain("\u{1b}[31mError:\u{1b}[0m no repository\r\n"),
            "Error: no repository\n"
        );
    }

    #[test]
    fn statuses_are_read_from_the_listing() {
        let listing = r#"{"tasks":[{"id":"a","status":"ready","title":"t"},{"id":"b","status":"pending"},{"title":"no id"}],"cursor":null}"#;
        let read = statuses(listing);
        assert_eq!(
            read,
            [("a".into(), "ready".into()), ("b".into(), "pending".into())]
        );
        assert_eq!(statuses("Error: nothing"), vec![]);
        assert_eq!(
            last_line("warn\n\nError: no such thing\n"),
            "Error: no such thing"
        );
        assert_eq!(last_line(""), "it gave no reason");
    }
}

//! The line under the message box: the user's own script, if they configured one.
//!
//! Without a script, front ends draw the built-in items chosen in the settings from what they
//! already know. With one, they ask here, and the script decides everything that is shown.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::Core;
use crate::agent::AgentEvent;
use crate::error::{Error, Result};

/// How long the script may run before the status line gives up on it.
const TIMEOUT: Duration = Duration::from_secs(5);
/// How much of the script's first line is shown.
const MAX_CHARS: usize = 300;

/// One built-in piece of the status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusItem {
    /// The task's branch.
    Branch,
    /// How many files the task changed, with lines added and removed.
    Changes,
    /// The model the session runs on.
    Model,
    /// How full the agent's context window is.
    Context,
    /// What the session has cost since its agent last started.
    Cost,
    /// The agent that runs the session.
    Agent,
    /// The task's worktree directory.
    Worktree,
    /// How long the session's agent has worked: its turns added up.
    Duration,
}

impl StatusItem {
    /// What a status line shows until the user chooses.
    #[must_use]
    pub fn defaults() -> Vec<Self> {
        vec![
            Self::Branch,
            Self::Changes,
            Self::Model,
            Self::Context,
            Self::Cost,
        ]
    }
}

fn failed(reason: impl std::fmt::Display) -> Error {
    Error::StatusLine(reason.to_string())
}

/// The first line of what the script printed, shortened to what fits a status line.
fn first_line(output: &[u8]) -> String {
    let text = String::from_utf8_lossy(output);
    let line = text.lines().next().unwrap_or_default().trim();
    line.chars().take(MAX_CHARS).collect()
}

/// Runs the user's script with the session's details as JSON on its standard input.
async fn run_script(command: &str, cwd: &Path, input: &Value) -> Result<String> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(failed)?;
    if let Some(mut stdin) = child.stdin.take() {
        // A script that never reads its input closes the pipe; that is its choice, not a failure.
        // One line, ended like a line, so line-based tools read it whole.
        let line = format!("{input}\n");
        stdin.write_all(line.as_bytes()).await.ok();
    }
    let finished = tokio::time::timeout(TIMEOUT, child.wait_with_output()).await;
    let output = finished
        .map_err(|_| failed("the script took longer than 5 seconds"))?
        .map_err(failed)?;
    if output.status.success() {
        return Ok(first_line(&output.stdout));
    }
    let complaint = first_line(&output.stderr);
    Err(failed(format!("the script failed: {complaint}")))
}

impl Core {
    /// What the user's status line script prints for a session, or `None` if no script is
    /// configured and the built-in items apply.
    ///
    /// The script runs with `sh -c` in the task's worktree and gets a JSON object on standard
    /// input: `session`, `task` and `project`, with the model, cost and context window use
    /// last reported.
    pub async fn status_line(&self, session_id: i64) -> Result<Option<String>> {
        let settings = self.settings()?;
        let command = settings.status_command.trim();
        if command.is_empty() {
            return Ok(None);
        }
        let (input, worktree) = self.status_input(session_id)?;
        run_script(command, &worktree, &input).await.map(Some)
    }

    /// Everything a status line script is told, and the directory it runs in.
    fn status_input(&self, session_id: i64) -> Result<(Value, std::path::PathBuf)> {
        let session = self.session(session_id)?;
        let store = self.store();
        let task = store.task_by_id(session.task_id)?;
        let task = task.ok_or(Error::SessionNotFound(session_id))?;
        let project = store.project_by_id(task.project_id)?;
        let model = match store.latest_event(session_id, "init")? {
            Some(AgentEvent::Init { model, .. }) => Some(model),
            _ => session.model.clone(),
        };
        let cost = match store.latest_event(session_id, "turn_done")? {
            Some(AgentEvent::TurnDone { cost_usd, .. }) => cost_usd,
            _ => None,
        };
        let context = match store.latest_event(session_id, "context_usage")? {
            Some(AgentEvent::ContextUsage { used, max, .. }) => json!({ "used": used, "max": max }),
            _ => Value::Null,
        };
        let input = json!({
            "session": {
                "id": session.id, "title": session.title, "agent": session.agent,
                "model": model, "supervision": session.supervision, "status": session.status,
                "cost_usd": cost, "context": context,
            },
            "task": { "name": task.name, "branch": task.branch, "worktree": task.worktree_path },
            "project": project.map(|p| json!({ "name": p.name, "repo": p.repo_path })),
        });
        Ok((input, task.worktree_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_first_line_is_shown_and_not_all_of_a_long_one() {
        assert_eq!(first_line(b"  main  +3 -1 \nsecond\n"), "main  +3 -1");
        assert_eq!(first_line(b""), "");
        let long = "x".repeat(1000);
        assert_eq!(first_line(long.as_bytes()).chars().count(), MAX_CHARS);
    }

    #[tokio::test]
    async fn the_script_reads_the_session_and_its_first_line_is_the_status() {
        let dir = tempfile::tempdir().expect("temp dir");
        let input = json!({ "task": { "branch": "me/fix" } });
        // Without jq: pull the branch out of the JSON with sed.
        let script = r#"sed -E 's/.*"branch":"([^"]*)".*/on \1/'; echo ignored"#;
        let shown = run_script(script, dir.path(), &input).await;
        assert_eq!(shown.expect("runs"), "on me/fix");
    }

    #[tokio::test]
    async fn a_failing_script_says_why() {
        let dir = tempfile::tempdir().expect("temp dir");
        let failed = run_script("echo no such thing >&2; exit 3", dir.path(), &json!({})).await;
        assert!(
            matches!(failed, Err(Error::StatusLine(reason)) if reason.contains("no such thing"))
        );
    }
}

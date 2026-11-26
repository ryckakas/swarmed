//! A session starting a new task for work that needs a branch of its own: a separate change, a
//! fix to another pull request, the next pull request of a stack. Without a tool for it, an
//! agent asked for such work finds no way to make a task and switches its own worktree instead.
//!
//! The task is made as the window's New task makes one ([`Core::task_start`]): the setup runs
//! in this process and shows in the task's setup panel, and the prompt goes once it finished
//! well. Its session is the app's from the start, so the user can watch it, stop it and answer
//! it like any other. A command ends with its turn and would take the setup along, so the tool
//! is answered only by a process that stays ([`Core::set_stays`]).

use std::sync::Arc;

use serde_json::Value;

use super::{Starting, left_by, refused};
use crate::agent::{AgentEvent, Auth};
use crate::environments::EnvironmentChoice;
use crate::error::{Error, Result};
use crate::sessions::{SessionOptions, title_from};
use crate::{Core, FirstSession, NewTaskOptions, TaskStarted, hooks};

/// What a session asks for in a task it starts.
#[derive(Debug, Clone, Copy, Default)]
pub struct PeerTask<'a> {
    /// The task's name. One that is taken gives way to the first free one, as a name the
    /// window makes up does.
    pub name: &'a str,
    /// What the task's first session is to do.
    pub prompt: &'a str,
    /// What its first session is called; named after the prompt if left out.
    pub title: Option<&'a str>,
    /// An existing branch to check out instead of a new one.
    pub branch: Option<&'a str>,
    /// A branch to start the new one from instead of the repository's base.
    pub from: Option<&'a str>,
    /// The role of its first session; without one, the role the asking session's starts by
    /// default (`roles.rs`).
    pub role: Option<&'a str>,
}

impl<'a> PeerTask<'a> {
    /// The task asked for in the arguments of a `start_task` call.
    fn from_call(arguments: &'a Value) -> Self {
        let word = |key: &str| {
            arguments
                .get(key)
                .and_then(Value::as_str)
                .filter(|word| !word.trim().is_empty())
        };
        Self {
            name: word("name").unwrap_or_default(),
            prompt: word("prompt").unwrap_or_default(),
            title: word("title"),
            branch: word("branch"),
            from: word("from"),
            role: word("role"),
        }
    }
}

impl Core {
    /// Starts a new task in the repository of session `from`'s task, with a session on the
    /// same agent and settings as `from`'s, whose prompt goes once the task is set up. The
    /// prompt is marked as `from`'s words, and tells the new session to report back to it.
    ///
    /// The new session works for `from`, but is not one `from` spawned: it is the first of a
    /// task of its own, and can start sessions and tasks itself, as the next link of a chain of
    /// pull requests does. It has the role asked for, or the one `from`'s own starts by default,
    /// and starts under the supervision that role holds, if it holds one. Who may start what is
    /// `may_start`'s: a role `from`'s may not start is refused, and so is a task asked
    /// for by a session with no role that another session's agent started, and one more while
    /// [`MAX_SPAWNED`](crate::MAX_SPAWNED) open sessions work for `from`. A chat starts none,
    /// having no repository. Must be called from within a tokio runtime.
    pub async fn peer_start_task(
        self: &Arc<Self>,
        from: i64,
        asked: PeerTask<'_>,
    ) -> Result<TaskStarted> {
        let asking = self.session(from)?;
        let role = self.may_start(&asking, asked.role, Starting::Task)?;
        self.room_for_another(from)?;
        if asked.prompt.trim().is_empty() {
            return Err(Error::EmptyMessage);
        }
        let own = self.task_of(&asking)?;
        let project = self
            .store()
            .project_by_id(own.project_id)?
            .ok_or(Error::SessionNotFound(from))?;
        if project.chats {
            return Err(refused(
                "a chat has no repository to start a task in; ask the user to start one",
            ));
        }
        let options = NewTaskOptions {
            existing_branch: asked.branch,
            from: asked.from,
            environment: EnvironmentChoice::Default,
            free_name: true,
            issue: None,
        };
        let note = format!(
            "(Session {from} started this task for that work, which is yours: this worktree and \
             its branch. When you are done, report what you did and found to it with the \
             `send_message` tool, to session {from}.)"
        );
        let first = FirstSession {
            options: SessionOptions {
                agent: Some(asking.agent.clone()),
                // The prompt's own words, not the mark of who wrote it, which every task
                // started this way would be called after.
                title: Some(
                    asked
                        .title
                        .map_or_else(|| title_from(asked.prompt), String::from),
                ),
                model: asking.model.clone(),
                supervision: self.supervision_without_role(&asking)?,
                effort: asking.effort,
                fast: asking.fast,
                role: role.map(|role| role.name.to_string()),
            },
            prompt: left_by(&asking, &format!("{}\n\n{note}", asked.prompt)),
            files: Vec::new(),
            sessions: Vec::new(),
            github: Vec::new(),
            images: Vec::new(),
        };
        let starting = self.task_start(
            &project.name,
            asked.name,
            options,
            first,
            Auth::default(),
            &hooks::discard,
        );
        // The setup goes on by itself, as the window's New task leaves it.
        let (mut started, _going) = starting.await?;
        // Before its agent starts, which is once the setup is done, so that it is told whom it
        // works for from the start.
        self.store()
            .session_set_parent(started.session.id, Some(from))?;
        started.session.parent = Some(from);
        if asked.title.is_none() {
            self.name_later(started.session.id, asked.prompt);
        }
        // Front ends learn of a task this process made only when told.
        self.live.publish_apart(AgentEvent::TreeChanged);
        Ok(started)
    }

    /// Answers a `start_task` call of session `from`: what was started, for its agent.
    pub(super) async fn peer_start_task_said(
        self: &Arc<Self>,
        from: i64,
        arguments: &Value,
    ) -> Result<String> {
        // The setup and the new session's agent run here, and would end with this process.
        if !self.stays {
            return Err(refused(
                "a session run from the command line cannot start tasks: the command ends with \
                 its turn, before the task is set up; ask the user to start one",
            ));
        }
        let asked = PeerTask::from_call(arguments);
        let started = self.peer_start_task(from, asked).await?;
        let task = &started.task;
        let what = started
            .session
            .role
            .as_deref()
            .map_or_else(String::new, |role| format!(" (role: {role})"));
        Ok(format!(
            "Started task `{}` on the branch `{}`, in {}, with session {}{what}. Its setup runs \
             first, and the prompt goes once it finished well; if the setup fails, the prompt \
             waits for the user. That session reports back to you with a message when it is \
             done.",
            task.name,
            task.branch,
            task.worktree_path.display(),
            started.session.id
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_is_read_with_blank_words_as_none() {
        let arguments = serde_json::json!({
            "name": "fix-pr", "prompt": "fix it", "title": " ", "branch": "pr/5028",
        });
        let asked = PeerTask::from_call(&arguments);
        assert_eq!((asked.name, asked.prompt), ("fix-pr", "fix it"));
        assert_eq!(
            (asked.title, asked.branch, asked.from),
            (None, Some("pr/5028"), None)
        );
        let empty = PeerTask::from_call(&Value::Null);
        assert_eq!((empty.name, empty.prompt), ("", ""));
    }
}

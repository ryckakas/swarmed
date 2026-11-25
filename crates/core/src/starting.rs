//! A task started from a prompt, as the window starts one.
//!
//! Its first session is made as soon as the task is written down, before its setup, with the
//! prompt waiting in its queue: a task never stands empty while it is set up, and the prompt is
//! never lost. The setup then goes on by itself, and sends the prompt when it finishes well. One
//! that fails or is stopped leaves it waiting, kept in the database, to go with the next setup
//! that finishes well or when the user sends it anyway ([`Core::task_start_anyway`]).

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tokio::task::JoinHandle;

use crate::agent::Auth;
use crate::attach::Attached;
use crate::chats::CHATS;
use crate::error::{Error, Result};
use crate::hooks::{self, OutputSink};
use crate::pasted::Pasted;
use crate::sessions::{Session, SessionOptions};
use crate::store::Task;
use crate::tasks::NewTaskOptions;
use crate::{Core, paths, with_files};

/// The first session of a task started from a prompt, and what it is sent.
#[derive(Debug, Clone, Default)]
pub struct FirstSession {
    /// How the session runs. Its title, if given, is the session's; without one it is named
    /// after the prompt, as a session is after its first message.
    pub options: SessionOptions,
    /// What it is sent.
    pub prompt: String,
    /// The files the prompt points the agent at, listed under it.
    pub files: Vec<PathBuf>,
    /// The sessions the prompt mentions, listed under it before the files: each one a session
    /// of the task's repository could read (`mentions.rs`).
    pub sessions: Vec<i64>,
    /// The pull requests and issues of the repository the prompt mentions, by number, listed
    /// after the sessions: each one read first ([`Core::project_github_read`]).
    pub github: Vec<u64>,
    /// Images pasted into the prompt, each in base64: kept as the session's own, as
    /// [`Core::session_images_keep`] keeps them, and listed after the files.
    pub images: Vec<String>,
}

/// A task just started, and its first session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskStarted {
    /// The task, as it was written down: its setup has only begun.
    pub task: Task,
    /// Its first session, whose prompt waits for the setup.
    pub session: Session,
}

/// The setup of a task just started, which goes on by itself. Dropping this leaves it going.
#[derive(Debug)]
pub struct SetupGoing(Going);

#[derive(Debug)]
enum Going {
    /// A chat, which has no setup.
    Done(Box<Task>),
    Running(JoinHandle<Result<Task>>),
}

impl SetupGoing {
    /// Waits until the setup is over: the task as it was set up, or what stopped it.
    pub async fn finished(self) -> Result<Task> {
        match self.0 {
            Going::Done(task) => Ok(*task),
            Going::Running(running) => running
                .await
                .map_err(|error| Error::Io(std::io::Error::other(error)))?,
        }
    }
}

impl Core {
    /// Starts a task from a prompt: makes the task as [`Core::task_new`] does, then its first
    /// session at once, with the prompt waiting in its queue, and returns both while the task
    /// is set up. The setup goes on by itself and, when it finishes well, starts the session's
    /// agent with `auth` and sends the prompt. One that fails or is stopped leaves the prompt
    /// waiting, kept also through a restart; [`Core::task_start_anyway`] sends it as things
    /// are, and [`Core::task_setup`] sets the task up again and sends it after.
    ///
    /// `sink` hears how making the task goes; what the setup prints arrives as
    /// [`crate::agent::AgentEvent::Setup`] and is kept in its log ([`Core::task_setup_log`]).
    /// In [`CHATS`] it makes a chat, which has no setup, and sends the prompt at once.
    ///
    /// A prompt with nothing in it, or naming a file that is not there, is refused before
    /// anything is made. Must be called from within a tokio runtime.
    pub async fn task_start(
        self: &Arc<Self>,
        project: &str,
        name: &str,
        options: NewTaskOptions<'_>,
        first: FirstSession,
        auth: Auth,
        sink: OutputSink<'_>,
    ) -> Result<(TaskStarted, SetupGoing)> {
        paths::validate_name(name)?;
        if first.prompt.trim().is_empty() {
            return Err(Error::EmptyMessage);
        }
        // Checked as the session will be once it is there, before anything is made.
        let prompt = self.mentioned_from(project, &first.prompt, &first.sessions, &first.github)?;
        let message = with_files(&prompt, &first.files)?;
        let images = Pasted::all(&first.images)?;
        if let Some(agent) = &first.options.agent {
            self.agents_now().backend(agent)?;
        }
        if project == CHATS {
            return self.chat_start(name, &options, &first, &images, auth).await;
        }
        let made = self.task_made(project, name, &options, sink).await?;
        let run = self.setup_begin(&made.task)?;
        let session = self.session_for_prompt(&made.task, &first.options, &message)?;
        let files = self.with_images(session.id, &first.files, &images)?;
        let message = with_files(&prompt, &files)?;
        self.keep_for_setup(made.task.id, session.id, &message)?;
        let started = TaskStarted {
            task: made.task.clone(),
            session,
        };
        let core = Arc::clone(self);
        let going = tokio::spawn(async move {
            core.set_up_made(made, &run, Some(auth), &hooks::discard)
                .await
        });
        Ok((started, SetupGoing(Going::Running(going))))
    }

    /// Starts a chat from a prompt: it has no setup, so the prompt goes at once.
    async fn chat_start(
        &self,
        name: &str,
        options: &NewTaskOptions<'_>,
        first: &FirstSession,
        images: &[Pasted],
        auth: Auth,
    ) -> Result<(TaskStarted, SetupGoing)> {
        let task = self.chat_new(name, options).await?;
        let message = with_files(&first.prompt, &first.files)?;
        let session = self.session_for_prompt(&task, &first.options, &message)?;
        let attached = Attached {
            files: self.with_images(session.id, &first.files, images)?,
            sessions: first.sessions.clone(),
            github: first.github.clone(),
            elements: Vec::new(),
        };
        self.session_message(session.id, &first.prompt, &attached, auth)?;
        let started = TaskStarted {
            task: task.clone(),
            session,
        };
        Ok((started, SetupGoing(Going::Done(Box::new(task)))))
    }

    /// Sends what the task's sessions kept waiting for its setup now, in the worktree as it
    /// is: for a setup that failed, was stopped or was cut short. Their agents are started with
    /// `auth`. Returns the sessions it went to. Refused while the task is set up, here or by
    /// another Swarmed, and when nothing waits ([`Error::NothingWaitsForSetup`]). Must be
    /// called from within a tokio runtime.
    pub fn task_start_anyway(&self, project: &str, name: &str, auth: &Auth) -> Result<Vec<i64>> {
        let (_, task) = self.repo_task(project, name)?;
        if self.setup_runs(&task) {
            return Err(Error::SetupRunning(task.name));
        }
        let waiting: Vec<i64> = self
            .store()
            .sessions(task.id)?
            .into_iter()
            .filter(|session| session.waits_for_setup)
            .map(|session| session.id)
            .collect();
        if waiting.is_empty() {
            return Err(Error::NothingWaitsForSetup(task.name));
        }
        for id in &waiting {
            self.session_resume_queue(*id, auth.clone())?;
        }
        Ok(waiting)
    }

    /// `files`, then the paths of `images` once they are kept as session `id`'s own.
    fn with_images(&self, id: i64, files: &[PathBuf], images: &[Pasted]) -> Result<Vec<PathBuf>> {
        let kept = self.images_kept(id, images)?;
        Ok(files.iter().cloned().chain(kept).collect())
    }
}

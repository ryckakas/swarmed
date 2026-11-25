//! Tasks started from a prompt, and what waits for a task's setup.

use std::sync::Arc;

use anyhow::Result;
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{
    Core, FirstSession, NewTaskOptions, SessionEvent, SessionOptions, SetupGoing, Task,
};
use tokio::sync::broadcast::Receiver;

use crate::session::follow_turn;
use crate::{aside, created, hook_line, say, setting_up};

/// What `swarmed task new` was asked to start the task's first session with.
pub(crate) struct Prompted {
    prompt: String,
    agent: Option<String>,
    model: Option<String>,
}

impl Prompted {
    pub(crate) fn new(prompt: String, agent: Option<String>, model: Option<String>) -> Self {
        Self {
            prompt,
            agent,
            model,
        }
    }
}

/// Makes a task and its first session at once, with the prompt waiting for the setup, shows
/// the setup as it goes, then the turn the prompt starts once the setup finished well.
/// Ctrl-C stops the setup, as it does for any; then the prompt stays waiting.
pub(crate) async fn start(
    core: &Arc<Core>,
    (project, name): (&str, &str),
    options: NewTaskOptions<'_>,
    prompted: Prompted,
) -> Result<()> {
    let first = FirstSession {
        options: SessionOptions {
            agent: prompted.agent,
            model: prompted.model,
            supervision: core.settings()?.default_supervision,
            ..SessionOptions::default()
        },
        prompt: prompted.prompt,
        files: vec![],
        sessions: vec![],
        github: vec![],
        images: vec![],
    };
    // Before anything is made, so that nothing it says is missed.
    let mut turn = core.subscribe();
    let told = core.subscribe();
    let (started, going) = core
        .task_start(project, name, options, first, Auth::default(), &hook_line)
        .await?;
    say(&created(&started.task));
    let session = started.session.id;
    say(&format!(
        "session {session}: its prompt waits for the setup"
    ));
    shown_until_set_up(core, told, started.task.id, going).await?;
    turn_if_sent(core, session, &mut turn).await
}

/// Runs the task's setup again and, once it finished well, shows the turn of a message that
/// waited for it.
pub(crate) async fn again(core: &Core, project: &str, name: &str) -> Result<()> {
    let waiting = waiting_sessions(core, project, name)?;
    let mut turn = core.subscribe();
    let setting = core.task_setup(project, name, Some(Auth::default()), &hook_line);
    setting_up(core, setting).await?;
    say(&format!("setup finished for `{name}`"));
    match waiting.first() {
        Some(id) => turn_if_sent(core, *id, &mut turn).await,
        None => Ok(()),
    }
}

/// Shows the turn of the message session `id` waited to send, if the setup's end sent it. One
/// that could not be sent, or whose agent was busy, started no turn here to wait for.
async fn turn_if_sent(core: &Core, id: i64, turn: &mut Receiver<SessionEvent>) -> Result<()> {
    if core.session(id)?.waits_for_setup {
        aside(&format!(
            "session {id} still waits to send its prompt: `swarmed task start-anyway` sends it"
        ));
        return Ok(());
    }
    follow_turn(core, id, turn).await
}

/// Sends what waits for the task's setup now, in the worktree as it is, and shows the turn it
/// starts.
pub(crate) async fn anyway(core: &Core, project: &str, name: &str) -> Result<()> {
    let mut turn = core.subscribe();
    let sent = core.task_start_anyway(project, name, &Auth::default())?;
    if sent.len() > 1 {
        aside("several sessions were sent what they waited to send; this shows the first");
    }
    match sent.first() {
        Some(id) => follow_turn(core, *id, &mut turn).await,
        None => Ok(()),
    }
}

/// Prints what the task's last setup printed, and how it ended.
pub(crate) fn log(core: &Core, project: &str, name: &str) -> Result<()> {
    let log = core.task_setup_log(project, name)?;
    say(log.text.trim_end());
    Ok(())
}

/// The sessions of a task whose first message waits for its setup.
fn waiting_sessions(core: &Core, project: &str, name: &str) -> Result<Vec<i64>> {
    let sessions = core.sessions(project, name)?;
    let waiting = sessions.iter().filter(|session| session.waits_for_setup);
    Ok(waiting.map(|session| session.id).collect())
}

/// Prints what the setup of task `task` prints until it is over, and returns how it ended.
async fn shown_until_set_up(
    core: &Core,
    mut told: Receiver<SessionEvent>,
    task: i64,
    going: SetupGoing,
) -> Result<Task> {
    let finished = setting_up(core, going.finished());
    let mut finished = std::pin::pin!(finished);
    loop {
        tokio::select! {
            done = &mut finished => {
                // What it said last may still be on its way.
                std::iter::from_fn(|| told.try_recv().ok()).for_each(|heard| shown(&heard, task));
                return done;
            },
            heard = told.recv() => if let Ok(heard) = heard { shown(&heard, task) },
        }
    }
}

/// Prints what a setup printed, if `heard` is a report of task `task`'s setup.
fn shown(heard: &SessionEvent, task: i64) {
    if let AgentEvent::Setup { progress } = &heard.event
        && progress.task_id == task
    {
        progress.output.lines().for_each(hook_line);
    }
}

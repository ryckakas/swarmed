//! A task's stack: what its `start` command keeps running, such as a dev server.
//!
//! `setup` has to finish before a task can be worked in. `start` is the opposite: it is
//! started once setup is done and left running, with what it prints kept in a log. It runs in
//! a process group of its own, so that stopping it takes everything it started with it. A
//! stack lives as long as Swarmed does: it is stopped when its task is removed and when
//! Swarmed quits, and can be started again from a front end.

use std::fs::File;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::process::Command as Signal;

use crate::config::ProjectConfig;
use crate::error::Result;
use crate::hooks::OutputSink;
use crate::reach::{self, Local};
use crate::store::{Project, Task};
use crate::{Core, lock};

/// How long a stack is given to stop by itself once asked to, before it is killed.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// How often a stack that was asked to stop is looked at again.
const STOP_POLL: Duration = Duration::from_millis(50);

/// How much of the end of a stack's log is given to a front end.
const LOG_TAIL_BYTES: usize = 64_000;

/// How far along a task's stack is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StackState {
    /// Its `start` command is not running.
    Stopped,
    /// Its `start` command runs, but nothing answers at the stack's address yet: it may still
    /// be installing or building what it serves.
    Starting,
    /// Something answers at the stack's address.
    Running,
}

/// A task's stack as a front end shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Reach {
    /// `None` for a task whose project has no `start` command.
    pub(crate) state: Option<StackState>,
    /// Where it can be opened in a browser, the first being the one to open by default.
    pub(crate) urls: Vec<String>,
}

impl Core {
    /// Starts the task's stack with its `start` command, unless it is running already. Says
    /// whether there is a stack at all: a task whose project has no `start` command has none.
    pub fn stack_start(&self, project: &str, task: &str) -> Result<bool> {
        let (project, task) = self.task(project, task)?;
        let config = self.config_in(&project.name, task.environment.as_deref())?;
        self.start_stack(&project, &config, &task)
    }

    pub(crate) fn start_stack(
        &self,
        project: &Project,
        config: &ProjectConfig,
        task: &Task,
    ) -> Result<bool> {
        if config.hooks.start.trim().is_empty() {
            return Ok(false);
        }
        if self.stack_running(task.id) {
            return Ok(true);
        }
        let (command, env) = self.hook_command(&config.hooks.start, project, config, task)?;
        let log = self.stack_log_file(task.id);
        if let Some(dir) = log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let out = File::create(&log)?;
        let err = out.try_clone()?;
        // Started with the standard library, not Tokio: a front end may ask for this from a
        // thread that has no runtime, such as the one a window's own commands run on.
        let child = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&task.worktree_path)
            .env("PATH", crate::shellenv::path())
            .envs(env)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            // A group of its own, so that everything it starts can be stopped with it.
            .process_group(0)
            .spawn()?;
        lock(&self.live.stacks).insert(task.id, child);
        Ok(true)
    }

    /// Whether the task's stack is running. One that has ended by itself is forgotten.
    pub(crate) fn stack_running(&self, task: i64) -> bool {
        let mut stacks = lock(&self.live.stacks);
        let ended = stacks
            .get_mut(&task)
            .is_some_and(|child| !matches!(child.try_wait(), Ok(None)));
        if ended {
            stacks.remove(&task);
        }
        stacks.contains_key(&task)
    }

    /// How far along the task's stack is and where it is opened. The project's `open_url`
    /// is what counts if it names one; otherwise the addresses on this machine that the stack
    /// has printed, once they answer.
    pub(crate) fn reach(&self, project: &Project, config: &ProjectConfig, task: &Task) -> Reach {
        let named = self.url_of(project, config, task).ok().flatten();
        let has_stack = !config.hooks.start.trim().is_empty();
        let running = has_stack && self.stack_running(task.id);
        if !running {
            return Reach {
                state: has_stack.then_some(StackState::Stopped),
                urls: named.into_iter().collect(),
            };
        }
        let (state, urls) = match named {
            Some(url) => (named_state(&url), vec![url]),
            None => self.printed(task.id),
        };
        Reach {
            state: Some(state),
            urls,
        }
    }

    /// The addresses the running stack printed that answer, and what that says of its state.
    fn printed(&self, task: i64) -> (StackState, Vec<String>) {
        let log = std::fs::read(self.stack_log_file(task)).unwrap_or_default();
        let from = log.len().saturating_sub(LOG_TAIL_BYTES);
        let found = reach::found_in(&String::from_utf8_lossy(&log[from..]));
        let urls: Vec<String> = found
            .into_iter()
            .filter(Local::answers)
            .map(|local| local.url)
            .collect();
        let state = if urls.is_empty() {
            StackState::Starting
        } else {
            StackState::Running
        };
        (state, urls)
    }

    /// Stops the task's stack: runs its `stop` command, if it has one, then ends whatever of
    /// the stack is still there.
    pub async fn stack_stop(&self, project: &str, task: &str, sink: OutputSink<'_>) -> Result<()> {
        let (project, task) = self.task(project, task)?;
        let config = self.config_in(&project.name, task.environment.as_deref())?;
        self.stop_stack(&project, &config, &task, sink).await;
        Ok(())
    }

    pub(crate) async fn stop_stack(
        &self,
        project: &Project,
        config: &ProjectConfig,
        task: &Task,
        sink: OutputSink<'_>,
    ) {
        let running = lock(&self.live.stacks).remove(&task.id);
        let Some(child) = running else {
            return;
        };
        let stop = &config.hooks.stop;
        if let Err(error) = self
            .run_hook("stop", stop, project, config, task, sink)
            .await
        {
            sink(&format!("stop failed, ending the stack anyway: {error}"));
        }
        end(child).await;
    }

    /// Ends every stack, for when Swarmed quits: nothing it started is left running unseen.
    pub async fn stacks_stop_all(&self) {
        let running: Vec<Child> = lock(&self.live.stacks)
            .drain()
            .map(|(_, child)| child)
            .collect();
        for child in running {
            end(child).await;
        }
    }

    /// The end of what the task's stack has printed since it was last started.
    pub fn stack_log(&self, project: &str, task: &str) -> Result<String> {
        let (_, task) = self.task(project, task)?;
        let Ok(bytes) = std::fs::read(self.stack_log_file(task.id)) else {
            return Ok(String::new());
        };
        let from = bytes.len().saturating_sub(LOG_TAIL_BYTES);
        Ok(String::from_utf8_lossy(&bytes[from..]).into_owned())
    }

    fn stack_log_file(&self, task: i64) -> PathBuf {
        let folder = self.paths.data_dir.join("run").join("stacks");
        folder.join(format!("{task}.log"))
    }
}

/// Whether a stack whose project names its address answers there. An address that is not on
/// this machine cannot be asked cheaply, so it is taken to be up.
fn named_state(url: &str) -> StackState {
    match reach::local(url) {
        Some(local) if !local.answers() => StackState::Starting,
        _ => StackState::Running,
    }
}

/// Asks the stack's whole process group to stop, waits a little, then kills what is left.
async fn end(mut child: Child) {
    // The group's id is that of the process that leads it; a minus addresses the group.
    let group = format!("-{}", child.id());
    signal("-TERM", &group).await;
    if gone_within(&mut child, STOP_GRACE).await {
        return;
    }
    signal("-KILL", &group).await;
    child.kill().ok();
    gone_within(&mut child, STOP_GRACE).await;
}

async fn signal(which: &str, group: &str) {
    let sent = Signal::new("kill").args([which, "--", group]).status();
    sent.await.ok();
}

/// Whether `child` ended within `limit`. Looking is also what reaps it.
async fn gone_within(child: &mut Child, limit: Duration) -> bool {
    let until = Instant::now() + limit;
    loop {
        if !matches!(child.try_wait(), Ok(None)) {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        tokio::time::sleep(STOP_POLL).await;
    }
}

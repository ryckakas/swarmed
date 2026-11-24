//! The `setup` hooks this process runs, by task: so that one can be stopped on purpose, and so
//! that how far it is can be told.
//!
//! A task's `setup` can take minutes. Removing the task meanwhile found the hook among what
//! runs in the worktree and killed it a few seconds later, and its end was then reported as a
//! failure of the hook's. A setup is kept here from when its task is written down until its
//! hook and what follows it are done, and its hook runs in a process group of its own: a
//! removal stops it first, with everything it started, gives it time to wind down, and its
//! end is said for what it was ([`Error::SetupStopped`]). Quitting stops the setups this
//! process runs too, and so does Ctrl-C on the command line, and so does the user.
//!
//! While it runs, a setup says where it is ([`SetupStep`]) and what it prints: published as
//! [`AgentEvent::Setup`] a few times a second, never stored, and written to a log in the data
//! directory that only the user can read (`run/setups/<task>.log`), since a hook may print
//! secrets. Each setup begins its log anew; a removal deletes it. A lock beside it
//! (`run/setups/<task>.lock`) is held for as long as the setup runs, so that a Swarmed can tell
//! a task being set up by another from one whose setup was cut short ([`SettingUp`]). A project
//! may limit how many of its setups run their hook at once here (`max_setups`): the others
//! wait for a place, and say so.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, watch};

use crate::agent::AgentEvent;
use crate::elsewhere::taken;
use crate::error::{Error, Result};
use crate::hooks::OutputSink;
use crate::sessions::Live;
use crate::store::{SetupStatus, Task};
use crate::{Core, lock};

/// How long a removal waits for a setup to be over once its hook was ended: one whose
/// process outlived being killed, stuck in the system, must not keep the removal for ever.
const OVER_WITHIN: Duration = Duration::from_secs(15);

/// How often what a setup printed is published. Line by line, a hook that prints thousands
/// at once would fill the event stream, and front ends would have to read everything again.
const REPORT_EVERY: Duration = Duration::from_millis(250);

/// How much printed and not yet published is published at once, whatever the time.
const REPORT_AT: usize = 32 * 1024;

/// The most of a setup's log handed to a front end: the end of it.
const LOG_TAIL: u64 = 256 * 1024;

/// The folder of the setups' logs and locks, in the data directory.
const SETUPS_DIR: &str = "run/setups";

/// Why a setup was stopped before it finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStop {
    /// Its task is being removed.
    Removed,
    /// Somebody asked: Ctrl-C on the command line, or Swarmed quitting.
    Asked,
    /// The user stopped it, from the window.
    User,
}

impl SetupStop {
    /// What [`Error::SetupStopped`] says of it.
    pub(crate) fn said(self) -> &'static str {
        match self {
            Self::Removed => "because its task was removed",
            Self::Asked => "before it finished",
            Self::User => "by the user",
        }
    }

    /// Why it was stopped, in a sentence of its own.
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::Removed => "Stopped because the task was removed.",
            Self::Asked => {
                "Stopped because Swarmed quit, or the command line that ran it was interrupted."
            }
            Self::User => "Stopped by you.",
        }
    }
}

/// Where a task's setup is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStep {
    /// Making the worktree ready for its hook: the snapshot of its environment is put in
    /// place, if there is one.
    Preparing,
    /// Waiting for a place: as many of the project's setups run their hook as its
    /// `max_setups` allows.
    Waiting,
    /// The `setup` hook runs.
    Hook,
    /// What setup made is kept as its environment's snapshot, for new tasks to start from.
    Keeping,
    /// The task's stack is being started.
    Stack,
}

/// How a setup ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SetupEnd {
    /// It finished well.
    Done,
    /// It failed. `reason` says why, in words.
    Failed {
        /// Why, in words.
        reason: String,
    },
    /// It was stopped before it finished.
    Stopped {
        /// What stopped it.
        why: SetupStop,
        /// The same, in words.
        reason: String,
    },
}

/// What a task's setup has come to, as it goes: what [`AgentEvent::Setup`] carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupProgress {
    /// The task being set up.
    pub task_id: i64,
    /// Where it is.
    pub step: SetupStep,
    /// When this setup began, in milliseconds since 1970: it tells one run from the next.
    pub began: u64,
    /// What it printed since the last report, each line ending in a newline, as its log has
    /// it.
    pub output: String,
    /// Where in the log `output` starts, in bytes.
    pub from: u64,
    /// Where in the log `output` ends, in bytes.
    pub to: u64,
    /// How it ended, on its last report.
    pub ended: Option<SetupEnd>,
}

/// How a task's setup stands while it is not over, as the sidebar shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SettingUp {
    /// This Swarmed sets it up.
    Here {
        /// Where it is.
        step: SetupStep,
        /// When it began, in milliseconds since 1970.
        began: u64,
        /// The last line it printed, if it printed any.
        latest: Option<String>,
    },
    /// Another Swarmed process on the same data sets it up.
    Elsewhere,
    /// It never finished, and nothing sets it up: the Swarmed that did ended first.
    Interrupted,
}

/// The end of a task's setup log, from where it starts on a line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetupLog {
    /// What it holds.
    pub text: String,
    /// Where in the log `text` ends, in bytes: output published from there on comes after it.
    pub end: u64,
}

/// The setups this process runs.
#[derive(Debug, Default)]
pub(crate) struct Setups {
    running: Mutex<HashMap<i64, Arc<Setup>>>,
    /// Set once every setup is to stop, as when Swarmed quits: one begun after that is
    /// stopped from its start.
    closed: AtomicBool,
    /// Told whenever a hook ends or a setup is over or stopped, any of which may free a place.
    places: Notify,
}

impl Setups {
    /// Moves `setup` on to its hook if a place is free for it: fewer than `limit` of its
    /// project's hooks run here, or there is no limit. Under the lock on the running setups,
    /// so that two waiting for the last place do not both take it.
    fn place_taken(&self, setup: &Setup, limit: u32) -> bool {
        let running = lock(&self.running);
        let busy = running
            .values()
            .filter(|other| other.project == setup.project && other.task != setup.task)
            .filter(|other| other.step() == SetupStep::Hook)
            .count();
        let free = limit == 0 || u32::try_from(busy).is_ok_and(|busy| busy < limit);
        if free {
            setup.step_to(SetupStep::Hook);
        }
        free
    }
}

/// One task's setup while it is under way.
#[derive(Debug)]
pub(crate) struct Setup {
    task: i64,
    project: i64,
    /// When it began, in milliseconds since 1970.
    began: u64,
    /// When it began, to say how long it took.
    started: Instant,
    state: Mutex<State>,
    /// Becomes `true` when the setup is over: its hook ended and what follows it done.
    over: watch::Sender<bool>,
    /// Where front ends hear how it goes.
    live: Arc<Live>,
    /// The log it writes, if one could be opened.
    log: Mutex<Option<File>>,
    /// The lock that says it runs, held until it is over.
    held: Option<File>,
}

#[derive(Debug)]
struct State {
    /// The hook's leading process, which leads a group of its own, while it runs.
    group: Option<u32>,
    /// Whether the hook has ended.
    hook_over: bool,
    /// Why it was stopped, once it was.
    stopped: Option<SetupStop>,
    /// Whether the stop came before the hook had ended: what the hook's end says then is
    /// what being stopped made of it.
    interrupted: bool,
    step: SetupStep,
    /// The last line it printed.
    latest: Option<String>,
    /// What it printed and was not yet published.
    unsaid: String,
    /// How much of the log is written.
    written: u64,
}

impl Setup {
    fn new(task: &Task, live: Arc<Live>, stopped: Option<SetupStop>) -> Self {
        let state = State {
            group: None,
            hook_over: false,
            interrupted: stopped.is_some(),
            stopped,
            step: SetupStep::Preparing,
            latest: None,
            unsaid: String::new(),
            written: 0,
        };
        Self {
            task: task.id,
            project: task.project_id,
            began: now_millis(),
            started: Instant::now(),
            state: Mutex::new(state),
            over: watch::Sender::new(false),
            live,
            log: Mutex::new(None),
            held: None,
        }
    }

    /// Why it was stopped, if it was.
    pub(crate) fn stopped(&self) -> Option<SetupStop> {
        lock(&self.state).stopped
    }

    /// Why it was stopped, if that came before its hook had ended. A stop that came after
    /// leaves the hook's own end standing.
    pub(crate) fn interrupted(&self) -> Option<SetupStop> {
        let state = lock(&self.state);
        state.stopped.filter(|_| state.interrupted)
    }

    /// Notes the process that leads the hook's group, and says whether the hook may go on:
    /// one stopped before it began is not.
    pub(crate) fn started(&self, group: u32) -> bool {
        let mut state = lock(&self.state);
        if state.stopped.is_some() {
            return false;
        }
        state.group = Some(group);
        true
    }

    /// Notes that the hook has ended, so that a stop after this signals nothing.
    pub(crate) fn hook_ended(&self) {
        let mut state = lock(&self.state);
        state.group = None;
        state.hook_over = true;
    }

    /// Marks it stopped for `why`, and hands over the hook's group to end, if one runs. A
    /// removal says more than having been asked, and is what it was stopped for from then on.
    fn stop(&self, why: SetupStop) -> Option<u32> {
        let mut state = lock(&self.state);
        if state.stopped.is_none() || why == SetupStop::Removed {
            state.stopped = Some(why);
        }
        if !state.hook_over {
            state.interrupted = true;
        }
        state.group.take()
    }

    /// Waits until the setup is over, for no longer than `limit`. Says whether it is.
    async fn over_within(&self, limit: Duration) -> bool {
        let mut over = self.over.subscribe();
        let waited = tokio::time::timeout(limit, over.wait_for(|over| *over)).await;
        // Only running out of time counts: a sender that is gone has nothing more to say,
        // and the setup is over.
        waited.is_ok()
    }

    fn step(&self) -> SetupStep {
        lock(&self.state).step
    }

    /// How it stands, for the sidebar.
    fn now(&self) -> SettingUp {
        let state = lock(&self.state);
        SettingUp::Here {
            step: state.step,
            began: self.began,
            latest: state.latest.clone(),
        }
    }

    /// Writes a line it printed, or one Swarmed says of it, to its log, and keeps it to be
    /// published with the next report.
    pub(crate) fn say(&self, line: &str) {
        let text = format!("{line}\n");
        if let Some(log) = lock(&self.log).as_mut() {
            // A log that cannot be written costs the setup nothing: what it printed is still
            // published.
            log.write_all(text.as_bytes()).ok();
        }
        let mut state = lock(&self.state);
        state.written += text.len() as u64;
        state.unsaid.push_str(&text);
        if !line.trim().is_empty() {
            state.latest = Some(line.to_string());
        }
        let full = state.unsaid.len() >= REPORT_AT;
        drop(state);
        if full {
            self.report(None);
        }
    }

    /// Moves it on to `step`, which is published at once.
    pub(crate) fn step_to(&self, step: SetupStep) {
        let changed = std::mem::replace(&mut lock(&self.state).step, step) != step;
        if changed {
            self.report(None);
        }
    }

    /// Publishes how it stands, with what it printed since the last report.
    fn report(&self, ended: Option<SetupEnd>) {
        let mut state = lock(&self.state);
        let output = std::mem::take(&mut state.unsaid);
        let to = state.written;
        let progress = SetupProgress {
            task_id: self.task,
            step: state.step,
            began: self.began,
            from: to - output.len() as u64,
            to,
            output,
            ended,
        };
        // Published before the state is let go of, so that reports go out in the order of
        // the log: a front end drops one that starts before where it already is.
        self.live.publish_apart(AgentEvent::Setup { progress });
        drop(state);
    }

    /// Publishes what it printed since the last report, if it printed anything.
    fn report_unsaid(&self) {
        if !lock(&self.state).unsaid.is_empty() {
            self.report(None);
        }
    }

    /// Says how it ended, in its log and to front ends: its last report.
    pub(crate) fn end(&self, ended: SetupEnd) {
        let took = crate::snapshots::seconds(self.started.elapsed());
        let line = match &ended {
            SetupEnd::Done => format!("setup finished in {took}"),
            SetupEnd::Failed { reason } | SetupEnd::Stopped { reason, .. } => {
                format!("setup ended after {took}: {reason}")
            }
        };
        self.say(&line);
        self.report(Some(ended));
    }

    /// Publishes what it prints every [`REPORT_EVERY`] until it is over. Without a runtime
    /// nothing does, and what it printed goes with its next step.
    fn keep_reporting(self: &Arc<Self>) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        runtime.spawn(report_until_over(Arc::clone(self)));
    }
}

/// Publishes what `setup` printed every [`REPORT_EVERY`], until it is over.
async fn report_until_over(setup: Arc<Setup>) {
    let mut over = setup.over.subscribe();
    // Running out of time is what goes on; the setup being over, or gone, ends it.
    while tokio::time::timeout(REPORT_EVERY, over.wait_for(|over| *over))
        .await
        .is_err()
    {
        setup.report_unsaid();
    }
}

/// A task's place among the running setups. Letting go of it says the setup is over, also
/// when what ran it was given up halfway, and lets go of the lock that says it runs.
pub(crate) struct SetupRun {
    setups: Arc<Setups>,
    task: i64,
    pub(crate) setup: Arc<Setup>,
}

impl Drop for SetupRun {
    fn drop(&mut self) {
        let mut running = lock(&self.setups.running);
        if running
            .get(&self.task)
            .is_some_and(|setup| Arc::ptr_eq(setup, &self.setup))
        {
            running.remove(&self.task);
        }
        drop(running);
        // Unlocked before it is closed, as in `let_go_run`: a process forked meanwhile holds
        // the file open until it starts its program.
        if let Some(held) = &self.setup.held {
            held.unlock().ok();
        }
        self.setup.report_unsaid();
        self.setup.live.queues.release_for_setup(self.task);
        self.setup.over.send_replace(true);
        self.setups.places.notify_waiters();
    }
}

impl Core {
    /// Takes the task's place among the running setups, for as long as the setup is under
    /// way, with the lock that tells other Swarmed processes so, and begins its log anew. The
    /// messages its sessions wait to send stay waiting until it is over. Refused while the
    /// task's setup already runs, here or in another Swarmed.
    pub(crate) fn setup_begin(&self, task: &Task) -> Result<SetupRun> {
        // Read before the setup has its place: nothing would take it out again if this failed.
        let sessions = self.store().session_ids(task.id)?;
        let mut running = lock(&self.setups.running);
        if running.contains_key(&task.id) {
            return Err(Error::SetupRunning(task.name.clone()));
        }
        let held = self.setup_lock(task)?;
        let closed = self.setups.closed.load(Ordering::SeqCst);
        let mut setup = Setup::new(
            task,
            Arc::clone(&self.live),
            closed.then_some(SetupStop::Asked),
        );
        setup.held = Some(held);
        *lock(&setup.log) = open_log(&self.setup_file(task.id, "log"));
        let setup = Arc::new(setup);
        running.insert(task.id, Arc::clone(&setup));
        drop(running);
        self.live.queues.hold_for_setup(task.id, &sessions);
        setup.keep_reporting();
        Ok(SetupRun {
            setups: Arc::clone(&self.setups),
            task: task.id,
            setup,
        })
    }

    /// Takes the lock that says the task's setup runs in this process. Refused while another
    /// process holds it.
    fn setup_lock(&self, task: &Task) -> Result<File> {
        let path = self.setup_file(task.id, "lock");
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)?;
        // Tried again for a moment: whoever looks whether it is held holds it for an instant.
        if taken(&file)? {
            Ok(file)
        } else {
            Err(Error::SetupRunning(task.name.clone()))
        }
    }

    /// One of the files of task `task`'s setup: its `log` or its `lock`.
    fn setup_file(&self, task: i64, kind: &str) -> PathBuf {
        let folder = self.paths.data_dir.join(SETUPS_DIR);
        folder.join(format!("{task}.{kind}"))
    }

    /// Deletes what the task's setups left: its log and its lock. Not while another process
    /// sets it up, which still writes them.
    pub(crate) fn forget_setup_files(&self, task: i64) {
        let lock_file = self.setup_file(task, "lock");
        if crate::elsewhere::held_elsewhere(&lock_file) {
            return;
        }
        std::fs::remove_file(self.setup_file(task, "log")).ok();
        std::fs::remove_file(lock_file).ok();
    }

    /// Waits until the setup may run its hook: right away when its project has no
    /// `max_setups`, else once fewer of the project's hooks than that run here. Meanwhile it
    /// says it waits. A setup stopped while it waits stops waiting.
    pub(crate) async fn setup_place(&self, setup: &Setup, limit: u32) {
        while !self.place_or_wait(setup, limit).await {}
    }

    /// Takes a place for the setup's hook, or waits until one may have been freed: `true` once
    /// it has one, or was stopped.
    async fn place_or_wait(&self, setup: &Setup, limit: u32) -> bool {
        let freed = self.setups.places.notified();
        tokio::pin!(freed);
        // Listening before looking, so that a place freed in between is not missed.
        freed.as_mut().enable();
        if setup.stopped().is_some() || self.setups.place_taken(setup, limit) {
            return true;
        }
        setup.step_to(SetupStep::Waiting);
        freed.await;
        false
    }

    /// Notes that a setup's hook has ended, which frees its place for one that waits.
    pub(crate) fn setup_hook_over(&self, setup: &Setup) {
        setup.hook_ended();
        self.setups.places.notify_waiters();
    }

    /// How the task's setup stands, if it is not over: set up here, by another Swarmed, or
    /// cut short with nothing setting it up. `None` for one that is over, well or not.
    pub(crate) fn setting_up(&self, task: &Task) -> Option<SettingUp> {
        if task.archived {
            return None;
        }
        if let Some(setup) = lock(&self.setups.running).get(&task.id) {
            return Some(setup.now());
        }
        if crate::elsewhere::held_elsewhere(&self.setup_file(task.id, "lock")) {
            return Some(SettingUp::Elsewhere);
        }
        (task.setup == SetupStatus::Pending).then_some(SettingUp::Interrupted)
    }

    /// Whether the task's setup runs, here or in another Swarmed.
    pub(crate) fn setup_runs(&self, task: &Task) -> bool {
        lock(&self.setups.running).contains_key(&task.id) || self.setup_runs_elsewhere(task)
    }

    /// Whether another Swarmed process sets the task up.
    pub(crate) fn setup_runs_elsewhere(&self, task: &Task) -> bool {
        !lock(&self.setups.running).contains_key(&task.id)
            && crate::elsewhere::held_elsewhere(&self.setup_file(task.id, "lock"))
    }

    /// The end of what the task's last setup printed, and what Swarmed said of it, from the
    /// start of a line. For a setup under way, only what has been published: what it prints
    /// from there on arrives as [`AgentEvent::Setup`], starting where this ends. A task never
    /// set up here has an empty log.
    pub fn task_setup_log(&self, project: &str, task: &str) -> Result<SetupLog> {
        let (_, task) = self.task(project, task)?;
        let published = lock(&self.setups.running).get(&task.id).map(|setup| {
            let state = lock(&setup.state);
            state.written - state.unsaid.len() as u64
        });
        let Ok(mut file) = File::open(self.setup_file(task.id, "log")) else {
            return Ok(SetupLog {
                text: String::new(),
                end: 0,
            });
        };
        let end = published.unwrap_or(file.metadata()?.len());
        let from = end.saturating_sub(LOG_TAIL);
        file.seek(SeekFrom::Start(from))?;
        let mut bytes = Vec::new();
        file.take(end - from).read_to_end(&mut bytes)?;
        // Cut into the middle of a line, the end begins at the next one.
        let start = if from == 0 {
            0
        } else {
            bytes
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(0, |at| at + 1)
        };
        let end = from + bytes.len() as u64;
        Ok(SetupLog {
            text: String::from_utf8_lossy(&bytes[start..]).into_owned(),
            end,
        })
    }

    /// Stops the task's setup, if this process runs one, and waits until it is over, so that
    /// nothing of it carries on behind a removal: no hook still writing in the worktree, and
    /// no stack started after it. Its hook is asked to stop and given time, as a stack is,
    /// then killed with what it started. Returns what is to be said if it would not end.
    pub(crate) async fn setup_stop_for_removal(
        &self,
        task: &Task,
        sink: OutputSink<'_>,
    ) -> Option<String> {
        let setup = lock(&self.setups.running).get(&task.id).cloned()?;
        sink("stopping setup, as the task is being removed");
        if let Some(group) = setup.stop(SetupStop::Removed) {
            crate::stacks::end_group(group).await;
        }
        self.setups.places.notify_waiters();
        (!setup.over_within(OVER_WITHIN).await)
            .then(|| "setup would not end, and was passed over".to_string())
    }

    /// Stops the task's setup, which leaves it to be set up again: its hook is asked to stop
    /// with what it started, given time, then killed, and nothing after it runs. Its sessions'
    /// messages stay waiting. Only a setup this process runs can be stopped here: one another
    /// Swarmed runs is refused ([`Error::SetupRunsElsewhere`]). Nothing is done if no setup
    /// runs.
    pub async fn task_setup_stop(&self, project: &str, name: &str) -> Result<()> {
        let (_, task) = self.repo_task(project, name)?;
        let running = lock(&self.setups.running).get(&task.id).cloned();
        if running.is_none() && self.setup_runs_elsewhere(&task) {
            return Err(Error::SetupRunsElsewhere(task.name));
        }
        let Some(setup) = running else {
            return Ok(());
        };
        if let Some(group) = setup.stop(SetupStop::User) {
            crate::stacks::end_group(group).await;
        }
        self.setups.places.notify_waiters();
        setup.over_within(OVER_WITHIN).await;
        Ok(())
    }

    /// Stops every setup this process runs, and any begun from now on: for when Swarmed
    /// quits, or the command line is interrupted. Each hook is ended with what it started;
    /// what ran it finishes by itself, as stopped.
    pub async fn setups_stop_all(&self) {
        self.setups.closed.store(true, Ordering::SeqCst);
        let running: Vec<Arc<Setup>> = lock(&self.setups.running).values().cloned().collect();
        let groups: Vec<u32> = running
            .iter()
            .filter_map(|setup| setup.stop(SetupStop::Asked))
            .collect();
        self.setups.places.notify_waiters();
        for group in groups {
            crate::stacks::end_group(group).await;
        }
    }
}

/// What a setup's end comes to, from what ran it returned.
pub(crate) fn setup_end<T>(result: &Result<T>) -> SetupEnd {
    match result {
        Ok(_) => SetupEnd::Done,
        Err(Error::SetupStopped { why, .. }) => SetupEnd::Stopped {
            why: *why,
            reason: why.reason().to_string(),
        },
        Err(error) => SetupEnd::Failed {
            reason: failure_said(error),
        },
    }
}

/// Why a setup failed, in words a user can act on. A signal is said for what it means: the
/// hook was ended from outside, which this Swarmed did not do, since its own stops end as
/// [`Error::SetupStopped`].
pub(crate) fn failure_said(error: &Error) -> String {
    let Error::HookFailed { status, .. } = error else {
        return format!("Setup could not run: {error}.");
    };
    let signal = status
        .strip_prefix("signal: ")
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|number| number.parse::<u32>().ok());
    match signal {
        Some(9) => "`setup` was killed (signal 9, SIGKILL) by something outside it: the \
                    system, which ends programs like this when memory runs low, or another \
                    program, such as another Swarmed removing the task. This Swarmed did not \
                    end it."
            .to_string(),
        Some(15) => "`setup` was asked to stop (signal 15, SIGTERM) by something outside it, \
                     such as the system shutting down or another program. This Swarmed did \
                     not end it."
            .to_string(),
        Some(signal) => format!(
            "`setup` was ended by signal {signal} from outside it. This Swarmed did not end it."
        ),
        None => {
            let code = status.strip_prefix("exit status: ").unwrap_or(status);
            format!("`setup` failed: it exited with status {code}. What it printed is in its log.")
        }
    }
}

/// The log of a setup, begun anew, readable by the user alone: a hook may print secrets.
/// `None` when it cannot be opened, which costs the setup nothing.
fn open_log(path: &PathBuf) -> Option<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true).mode(0o600);
    options.open(path).ok()
}

/// Now, in milliseconds since 1970.
fn now_millis() -> u64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> Task {
        Task {
            id: 1,
            project_id: 1,
            name: "one".to_string(),
            branch: "one".to_string(),
            base_ref: "main".to_string(),
            worktree_path: PathBuf::from("/nowhere"),
            slot: 1,
            setup: SetupStatus::Pending,
            setup_reason: None,
            archived: false,
            environment: None,
            pinned: false,
        }
    }

    fn setup(stopped: Option<SetupStop>) -> Setup {
        Setup::new(&task(), Arc::default(), stopped)
    }

    #[test]
    fn a_stop_before_the_hook_ended_interrupts_it_and_one_after_does_not() {
        let setup = setup(None);
        assert!(setup.started(7));
        setup.hook_ended();
        assert_eq!(setup.stop(SetupStop::Asked), None);
        assert_eq!(setup.stopped(), Some(SetupStop::Asked));
        assert_eq!(setup.interrupted(), None);

        let setup = self::setup(None);
        assert!(setup.started(7));
        assert_eq!(setup.stop(SetupStop::Asked), Some(7));
        // The group is handed over once: a second stop has nothing left to end.
        assert_eq!(setup.stop(SetupStop::Removed), None);
        assert_eq!(setup.interrupted(), Some(SetupStop::Removed));
    }

    #[test]
    fn a_setup_stopped_before_its_hook_began_does_not_begin_it() {
        let setup = setup(None);
        assert_eq!(setup.stop(SetupStop::Removed), None);
        assert!(!setup.started(7));
        assert_eq!(setup.interrupted(), Some(SetupStop::Removed));

        let closed = self::setup(Some(SetupStop::Asked));
        assert!(!closed.started(7));
        assert_eq!(closed.interrupted(), Some(SetupStop::Asked));
    }

    #[test]
    fn being_asked_after_a_removal_still_says_removed() {
        let setup = setup(None);
        setup.stop(SetupStop::Removed);
        setup.stop(SetupStop::Asked);
        assert_eq!(setup.stopped(), Some(SetupStop::Removed));
    }

    #[test]
    fn what_a_setup_prints_is_published_with_where_it_lies_in_its_log() {
        let live: Arc<Live> = Arc::default();
        let mut heard = live.bus.subscribe();
        let setup = Setup::new(&task(), Arc::clone(&live), None);
        setup.say("installing");
        setup.say("");
        setup.step_to(SetupStep::Hook);
        // The same step again says nothing new.
        setup.step_to(SetupStep::Hook);
        setup.say("done");
        setup.end(SetupEnd::Done);

        let reports: Vec<SetupProgress> = std::iter::from_fn(|| heard.try_recv().ok())
            .filter_map(|published| match published.event {
                AgentEvent::Setup { progress } => Some(progress),
                _ => None,
            })
            .collect();
        assert_eq!(reports.len(), 2, "{reports:?}");
        assert_eq!(reports[0].output, "installing\n\n");
        assert_eq!((reports[0].from, reports[0].to), (0, 12));
        assert!(reports[1].output.starts_with("done\nsetup finished in "));
        assert_eq!(reports[1].from, 12);
        assert_eq!(reports[1].ended, Some(SetupEnd::Done));
        // The last line that says something is what the sidebar shows.
        let SettingUp::Here { latest, .. } = setup.now() else {
            panic!("a setup under way here");
        };
        assert!(latest.is_some_and(|line| line.starts_with("setup finished")));
    }

    #[test]
    fn a_hook_killed_from_outside_is_said_to_be_so() {
        let killed = Error::HookFailed {
            hook: "setup",
            status: "signal: 9 (SIGKILL)".to_string(),
        };
        let said = failure_said(&killed);
        assert!(
            said.contains("signal 9") && said.contains("did not end it"),
            "{said}"
        );
        let failed = Error::HookFailed {
            hook: "setup",
            status: "exit status: 3".to_string(),
        };
        assert!(failure_said(&failed).contains("exited with status 3"));
        let stopped: Result<()> = Err(Error::SetupStopped {
            task: "one".to_string(),
            why: SetupStop::User,
        });
        assert_eq!(
            setup_end(&stopped),
            SetupEnd::Stopped {
                why: SetupStop::User,
                reason: "Stopped by you.".to_string()
            }
        );
    }
}

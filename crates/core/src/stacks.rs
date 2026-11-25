//! A task's stack: what its `start` command keeps running, such as a dev server.
//!
//! `setup` has to finish before a task can be worked in. `start` is the opposite: it is
//! started once setup is done and left running, with what it prints kept in a log. It runs in
//! a process group of its own, and stopping it takes everything started from it along, also
//! what set itself apart in a group of its own, as the services of some stacks do. A stack is
//! stopped when its task is removed and when Swarmed quits, and can be started again from a
//! front end.
//!
//! Which process a task's stack is, is also written down beside its log. A Swarmed that did
//! not get to stop its stacks, because it crashed or was killed, finds them again that way
//! when it is next started: they count as running, are not started a second time over
//! themselves, and can be stopped.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::process::Command as Signal;

use crate::config::ProjectConfig;
use crate::error::{Error, Result};
use crate::hooks::OutputSink;
use crate::reach::{self, Local};
use crate::store::{Project, Task};
use crate::{Core, lock};

/// How long a stack is given to stop by itself once asked to, before it is killed. Long
/// enough for one that has services of its own to wind down first.
const STOP_GRACE: Duration = Duration::from_secs(15);

/// How long what a stack left behind is given, once asked, before it is killed too. Tools
/// that run others wind them down first: devenv takes up to five seconds.
const LEFTOVER_GRACE: Duration = Duration::from_secs(10);

/// How long a `stop` or `teardown` may run when a stack is stopped or a task removed. One that
/// takes longer, or leaves something behind that holds its output open, is not waited for:
/// it is killed with what it started, and what it was to stop is ended regardless.
pub(crate) const HOOK_LIMIT: Duration = Duration::from_secs(60);

/// How often a stack that was asked to stop is looked at again.
const STOP_POLL: Duration = Duration::from_millis(50);

/// The most of a stack's log that is read on in one go. What is beyond it comes next time.
const LOG_STEP_BYTES: u64 = 1_000_000;

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
    /// A put away task comes back once its stack starts.
    pub fn stack_start(&self, project: &str, task: &str) -> Result<bool> {
        let (project, task) = self.repo_task(project, task)?;
        let config = self.config_in(&project.name, task.environment.as_deref())?;
        let started = self.start_stack(&project, &config, &task)?;
        if started {
            self.bring_back(task.id)?;
        }
        Ok(started)
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
        self.room_for_a_stack(project, config, task)?;
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
        self.keep_stack(task.id, child.id());
        lock(&self.live.stacks).insert(task.id, child);
        Ok(true)
    }

    /// Refuses when as many of the project's stacks run as `max_stacks` allows (the task's
    /// environment's, or else the project's), counting the ones an earlier Swarmed left behind.
    fn room_for_a_stack(
        &self,
        project: &Project,
        config: &ProjectConfig,
        task: &Task,
    ) -> Result<()> {
        let others = self.store().tasks(project.id, false)?;
        let running = others
            .iter()
            .filter(|other| other.id != task.id && self.stack_running(other.id))
            .count();
        if u32::try_from(running).unwrap_or(u32::MAX) < config.max_stacks {
            return Ok(());
        }
        Err(Error::TooManyStacks {
            project: project.name.clone(),
            limit: config.max_stacks,
        })
    }

    /// Writes down which process the task's stack is, and when it started: a process id is
    /// handed out again sooner or later, and the next holder of it is no stack of ours.
    fn keep_stack(&self, task: i64, pid: u32) {
        let Some(started) = started_at(pid) else {
            return;
        };
        std::fs::write(self.stack_pid_file(task), format!("{pid}\n{started}\n")).ok();
    }

    /// The process that was written down as the task's stack, if it is still that process.
    /// What was written is forgotten otherwise.
    fn kept_stack(&self, task: i64) -> Option<u32> {
        let file = self.stack_pid_file(task);
        let written = std::fs::read_to_string(&file).ok()?;
        let (pid, started) = written.trim().split_once('\n')?;
        let pid: u32 = pid.trim().parse().ok()?;
        let started = started.trim();
        if started_at(pid).as_deref() == Some(started) {
            return Some(pid);
        }
        // An earlier Swarmed wrote the time in the words of its own language and time zone.
        // If they are this one's too, the stack is still known, and is written down anew.
        if started_at_in_local_words(pid).as_deref() == Some(started) {
            self.keep_stack(task, pid);
            return Some(pid);
        }
        std::fs::remove_file(file).ok();
        None
    }

    /// Whether the task's stack is running: one this Swarmed started, or one an earlier
    /// Swarmed left behind. One that has ended by itself is forgotten.
    pub(crate) fn stack_running(&self, task: i64) -> bool {
        let mut stacks = lock(&self.live.stacks);
        let ended = stacks
            .get_mut(&task)
            .is_some_and(|child| !matches!(child.try_wait(), Ok(None)));
        if ended {
            stacks.remove(&task);
            std::fs::remove_file(self.stack_pid_file(task)).ok();
        }
        stacks.contains_key(&task) || self.kept_stack(task).is_some()
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
        let (project, task) = self.repo_task(project, task)?;
        let config = self.config_in(&project.name, task.environment.as_deref())?;
        for note in self.stop_stack(&project, &config, &task, sink).await {
            sink(&note);
        }
        Ok(())
    }

    /// Stops the task's stack, and returns what went wrong on the way, to be said: a
    /// `stop` that failed or took longer than [`HOOK_LIMIT`], after which the stack is ended
    /// anyway, and a stack that would not end even when killed, which is left running and
    /// stays written down.
    pub(crate) async fn stop_stack(
        &self,
        project: &Project,
        config: &ProjectConfig,
        task: &Task,
        sink: OutputSink<'_>,
    ) -> Vec<String> {
        let started = lock(&self.live.stacks).remove(&task.id);
        let stack = match started {
            Some(child) => Stack::Started(child),
            None => match self.kept_stack(task.id) {
                Some(pid) => Stack::Found(pid),
                None => return vec![],
            },
        };
        let stop = &config.hooks.stop;
        let stopping = self.run_hook_limited("stop", stop, project, config, task, sink);
        let mut notes: Vec<String> = match stopping.await {
            Some(Ok(())) => vec![],
            Some(Err(error)) => vec![format!(
                "stop failed, so the stack was ended without it: {error}"
            )],
            None => {
                vec!["stop took longer than a minute, so the stack was ended without it".into()]
            }
        };
        let pid = stack.pid();
        if end(stack).await {
            std::fs::remove_file(self.stack_pid_file(task.id)).ok();
        } else {
            notes.push(format!(
                "the stack (process {pid}) would not end, and is left running"
            ));
        }
        notes
    }

    /// Ends the stacks this process started, and no others: a second Swarmed running beside
    /// this one, such as the app beside a command, keeps its own.
    pub async fn stacks_stop_started(&self) {
        let started: Vec<(i64, Child)> = lock(&self.live.stacks).drain().collect();
        for (task, child) in started {
            let gone = end(Stack::Started(child)).await;
            self.forget_stack_if(task, gone);
        }
    }

    /// Ends every stack, for when Swarmed quits: nothing it started is left running unseen,
    /// and neither is what an earlier Swarmed left behind.
    pub async fn stacks_stop_all(&self) {
        self.stacks_stop_started().await;
        for task in self.kept_tasks() {
            let gone = match self.kept_stack(task) {
                Some(pid) => end(Stack::Found(pid)).await,
                None => true,
            };
            self.forget_stack_if(task, gone);
        }
    }

    /// Forgets the stack written down for `task` if it is gone. One that would not end stays
    /// written down, for the next Swarmed to find.
    fn forget_stack_if(&self, task: i64, gone: bool) {
        if gone {
            std::fs::remove_file(self.stack_pid_file(task)).ok();
        }
    }

    /// The tasks a stack is written down for.
    fn kept_tasks(&self) -> Vec<i64> {
        let folder = self.paths.data_dir.join("run").join("stacks");
        let Ok(entries) = std::fs::read_dir(folder) else {
            return vec![];
        };
        let task = |entry: std::fs::DirEntry| {
            let name = entry.file_name();
            name.to_string_lossy().strip_suffix(".pid")?.parse().ok()
        };
        entries.flatten().filter_map(task).collect()
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

    /// What the task's stack has printed from byte `from` of its log on, and the place to ask
    /// from next time. A log that is shorter than `from` was begun anew, and is read from its
    /// start. A great deal that is new comes in several goes.
    pub fn stack_log_from(&self, project: &str, task: &str, from: u64) -> Result<(String, u64)> {
        let (_, task) = self.task(project, task)?;
        let Ok(mut file) = File::open(self.stack_log_file(task.id)) else {
            return Ok((String::new(), 0));
        };
        let from = if from > file.metadata()?.len() {
            0
        } else {
            from
        };
        file.seek(SeekFrom::Start(from))?;
        let mut bytes = Vec::new();
        file.take(LOG_STEP_BYTES).read_to_end(&mut bytes)?;
        // A character that is only half written yet is left for the next time.
        let whole = match std::str::from_utf8(&bytes) {
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            _ => bytes.len(),
        };
        let text = String::from_utf8_lossy(&bytes[..whole]).into_owned();
        Ok((text, from + whole as u64))
    }

    /// Whether the task's stack is running now. One that has ended by itself is not.
    pub fn stack_is_running(&self, project: &str, task: &str) -> Result<bool> {
        let (_, task) = self.task(project, task)?;
        Ok(self.stack_running(task.id))
    }

    fn stack_log_file(&self, task: i64) -> PathBuf {
        let folder = self.paths.data_dir.join("run").join("stacks");
        folder.join(format!("{task}.log"))
    }

    fn stack_pid_file(&self, task: i64) -> PathBuf {
        self.stack_log_file(task).with_extension("pid")
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

/// A running stack: one this Swarmed started and so can wait for, or one it found.
enum Stack {
    Started(Child),
    Found(u32),
}

impl Stack {
    fn pid(&self) -> u32 {
        match self {
            Self::Started(child) => child.id(),
            Self::Found(pid) => *pid,
        }
    }

    /// Whether its leading process has ended. Looking is also what reaps one of our own.
    async fn is_gone(&mut self) -> bool {
        match self {
            Self::Started(child) => !matches!(child.try_wait(), Ok(None)),
            Self::Found(pid) => !alive(*pid).await,
        }
    }
}

/// Whether the stack's leading process ended within `limit`.
async fn gone_within(stack: &mut Stack, limit: Duration) -> bool {
    let until = Instant::now() + limit;
    loop {
        if stack.is_gone().await {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        tokio::time::sleep(STOP_POLL).await;
    }
}

/// When the process `pid` started, as `ps` words it; `None` if there is no such process.
///
/// `ps` words a time in the language and the time zone it is run in. Both are fixed here, so
/// that whoever reads the time back gets the words that were written, whatever it runs in.
fn started_at(pid: u32) -> Option<String> {
    let mut ps = Command::new("ps");
    ps.env("LC_ALL", "C").env("TZ", "UTC");
    start_time(ps, pid)
}

/// The same in the language and time zone of this process, as it was written down before
/// the two were fixed.
fn started_at_in_local_words(pid: u32) -> Option<String> {
    start_time(Command::new("ps"), pid)
}

fn start_time(mut ps: Command, pid: u32) -> Option<String> {
    let asked = ps
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let started = String::from_utf8_lossy(&asked.stdout).trim().to_string();
    (!started.is_empty()).then_some(started)
}

/// Whether there is a process `pid` that still runs. One that has ended and only waits for
/// whoever started it to take note does not count.
async fn alive(pid: u32) -> bool {
    let asked = Signal::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .stderr(Stdio::null())
        .output();
    let Ok(asked) = asked.await else {
        return false;
    };
    let state = String::from_utf8_lossy(&asked.stdout);
    let state = state.trim();
    !state.is_empty() && !state.starts_with('Z')
}

/// Every process started from `root`, at whatever remove, as they are now.
async fn started_from(root: u32) -> Vec<u32> {
    let listing = Signal::new("ps").args(["-axo", "pid=,ppid="]).output();
    let Ok(listing) = listing.await else {
        return vec![];
    };
    let listing = String::from_utf8_lossy(&listing.stdout);
    let pair = |line: &str| {
        let mut numbers = line.split_whitespace().map(str::parse::<u32>);
        Some((numbers.next()?.ok()?, numbers.next()?.ok()?))
    };
    below(&listing.lines().filter_map(pair).collect::<Vec<_>>(), root)
}

/// The processes below `root` among `processes`, each given with its parent.
fn below(processes: &[(u32, u32)], root: u32) -> Vec<u32> {
    let mut found: Vec<u32> = vec![];
    let mut waiting = vec![root];
    while let Some(parent) = waiting.pop() {
        // A process id met twice must not send this round in circles.
        let new = |(pid, of): &&(u32, u32)| *of == parent && *pid != root && !found.contains(pid);
        let children: Vec<u32> = processes.iter().filter(new).map(|(pid, _)| *pid).collect();
        found.extend(&children);
        waiting.extend(children);
    }
    found
}

/// Ends a stack. Its whole process group is asked to stop and given time, since a stack
/// with services of its own winds them down itself; then what is left of the group is killed,
/// and so is whatever it started that set itself apart and is still there. Says whether the
/// leading process is gone: one stuck in the system can outlive being killed.
async fn end(mut stack: Stack) -> bool {
    let started = started_from(stack.pid()).await;
    // The group's id is that of the process that leads it; a minus addresses the group.
    let group = format!("-{}", stack.pid());
    signal("-TERM", &group).await;
    let mut gone = gone_within(&mut stack, STOP_GRACE).await;
    if !gone {
        signal("-KILL", &group).await;
        if let Stack::Started(child) = &mut stack {
            child.kill().ok();
        }
        gone = gone_within(&mut stack, STOP_GRACE).await;
    }
    end_leftovers(started).await;
    gone
}

/// Ends the process group `leader` leads, with whatever it started, as a stack is ended: a
/// task's `setup` stopped while it runs. Says whether the leading process is gone.
pub(crate) async fn end_group(leader: u32) -> bool {
    end(Stack::Found(leader)).await
}

/// Ends what a stack started and did not take along, or whatever else is to go: asked
/// first, then killed. Returns what outlived even that.
pub(crate) async fn end_leftovers(started: Vec<u32>) -> Vec<u32> {
    let mut left = still_alive(started).await;
    for pid in &left {
        signal("-TERM", &pid.to_string()).await;
    }
    let until = Instant::now() + LEFTOVER_GRACE;
    while !left.is_empty() && Instant::now() < until {
        tokio::time::sleep(STOP_POLL).await;
        left = still_alive(left).await;
    }
    for pid in &left {
        signal("-KILL", &pid.to_string()).await;
    }
    // A killed process goes at once, unless it is stuck in the system, waiting on a disk.
    let until = Instant::now() + LEFTOVER_GRACE;
    while !left.is_empty() && Instant::now() < until {
        tokio::time::sleep(STOP_POLL).await;
        left = still_alive(left).await;
    }
    left
}

async fn still_alive(pids: Vec<u32>) -> Vec<u32> {
    let mut left = vec![];
    for pid in pids {
        if alive(pid).await {
            left.push(pid);
        }
    }
    left
}

async fn signal(which: &str, target: &str) {
    let sent = Signal::new("kill")
        .args([which, "--", target])
        .stderr(Stdio::null())
        .status();
    sent.await.ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_started_from_a_process_is_found_at_any_remove() {
        // 10 started 11 and 12, 12 started 13; 20 and its child have nothing to do with it.
        let processes = [(10, 1), (11, 10), (12, 10), (13, 12), (20, 1), (21, 20)];
        let mut found = below(&processes, 10);
        found.sort_unstable();
        assert_eq!(found, [11, 12, 13]);
        assert_eq!(below(&processes, 13), Vec::<u32>::new());
    }
}

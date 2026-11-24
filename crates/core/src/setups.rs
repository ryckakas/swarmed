//! The `setup` hooks this process runs, by task, so that one can be stopped on purpose.
//!
//! A task's `setup` can take minutes. Removing the task meanwhile found the hook among what
//! runs in the worktree and killed it a few seconds later, and its end was then reported as a
//! failure of the hook's. A setup is kept here from when its task is written down until its
//! hook and what follows it are done, and its hook runs in a process group of its own: a
//! removal stops it first, with everything it started, gives it time to wind down, and its
//! end is said for what it was ([`Error::SetupStopped`]). Quitting stops the setups this
//! process runs too, and so does Ctrl-C on the command line.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::watch;

use crate::error::{Error, Result};
use crate::hooks::OutputSink;
use crate::store::Task;
use crate::{Core, lock};

/// How long a removal waits for a setup to be over once its hook was ended: one whose
/// process outlived being killed, stuck in the system, must not keep the removal for ever.
const OVER_WITHIN: Duration = Duration::from_secs(15);

/// Why a setup was stopped before it finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStop {
    /// Its task is being removed.
    Removed,
    /// Somebody asked: Ctrl-C on the command line, or Swarmed quitting.
    Asked,
}

impl SetupStop {
    /// What [`Error::SetupStopped`] says of it.
    pub(crate) fn said(self) -> &'static str {
        match self {
            Self::Removed => "because its task was removed",
            Self::Asked => "before it finished",
        }
    }
}

/// The setups this process runs.
#[derive(Debug, Default)]
pub(crate) struct Setups {
    running: Mutex<HashMap<i64, Arc<Setup>>>,
    /// Set once every setup is to stop, as when Swarmed quits: one begun after that is
    /// stopped from its start.
    closed: AtomicBool,
}

/// One task's setup while it is under way.
#[derive(Debug)]
pub(crate) struct Setup {
    state: Mutex<State>,
    /// Becomes `true` when the setup is over: its hook ended and what follows it done.
    over: watch::Sender<bool>,
}

#[derive(Debug, Default)]
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
}

impl Setup {
    fn new(stopped: Option<SetupStop>) -> Self {
        let state = State {
            interrupted: stopped.is_some(),
            stopped,
            ..State::default()
        };
        Self {
            state: Mutex::new(state),
            over: watch::Sender::new(false),
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
}

/// A task's place among the running setups. Letting go of it says the setup is over, also
/// when what ran it was given up halfway.
pub(crate) struct SetupRun<'a> {
    setups: &'a Setups,
    task: i64,
    pub(crate) setup: Arc<Setup>,
}

impl Drop for SetupRun<'_> {
    fn drop(&mut self) {
        let mut running = lock(&self.setups.running);
        if running
            .get(&self.task)
            .is_some_and(|setup| Arc::ptr_eq(setup, &self.setup))
        {
            running.remove(&self.task);
        }
        drop(running);
        self.setup.over.send_replace(true);
    }
}

impl Core {
    /// Takes the task's place among the running setups, for as long as the setup is under
    /// way. Refused while the task's setup already runs here.
    pub(crate) fn setup_begin(&self, task: &Task) -> Result<SetupRun<'_>> {
        let mut running = lock(&self.setups.running);
        if running.contains_key(&task.id) {
            return Err(Error::SetupRunning(task.name.clone()));
        }
        let closed = self.setups.closed.load(Ordering::SeqCst);
        let setup = Arc::new(Setup::new(closed.then_some(SetupStop::Asked)));
        running.insert(task.id, Arc::clone(&setup));
        Ok(SetupRun {
            setups: &self.setups,
            task: task.id,
            setup,
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
        (!setup.over_within(OVER_WITHIN).await)
            .then(|| "setup would not end, and was passed over".to_string())
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
        for group in groups {
            crate::stacks::end_group(group).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stop_before_the_hook_ended_interrupts_it_and_one_after_does_not() {
        let setup = Setup::new(None);
        assert!(setup.started(7));
        setup.hook_ended();
        assert_eq!(setup.stop(SetupStop::Asked), None);
        assert_eq!(setup.stopped(), Some(SetupStop::Asked));
        assert_eq!(setup.interrupted(), None);

        let setup = Setup::new(None);
        assert!(setup.started(7));
        assert_eq!(setup.stop(SetupStop::Asked), Some(7));
        // The group is handed over once: a second stop has nothing left to end.
        assert_eq!(setup.stop(SetupStop::Removed), None);
        assert_eq!(setup.interrupted(), Some(SetupStop::Removed));
    }

    #[test]
    fn a_setup_stopped_before_its_hook_began_does_not_begin_it() {
        let setup = Setup::new(None);
        assert_eq!(setup.stop(SetupStop::Removed), None);
        assert!(!setup.started(7));
        assert_eq!(setup.interrupted(), Some(SetupStop::Removed));

        let closed = Setup::new(Some(SetupStop::Asked));
        assert!(!closed.started(7));
        assert_eq!(closed.interrupted(), Some(SetupStop::Asked));
    }

    #[test]
    fn being_asked_after_a_removal_still_says_removed() {
        let setup = Setup::new(None);
        setup.stop(SetupStop::Removed);
        setup.stop(SetupStop::Asked);
        assert_eq!(setup.stopped(), Some(SetupStop::Removed));
    }
}

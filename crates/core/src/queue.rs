//! Messages the user wrote while an agent was busy, waiting for its turn to end.
//!
//! A queue belongs to a session and lives only as long as the core does. When a turn ends well,
//! or the user stops it, the first message is sent as if the user had just typed it; a turn
//! that failed leaves the queue as it is, for the user to decide, and so does an agent that
//! was stopped. The user can also send a waiting message at once, which puts it first and
//! stops the turn under way. Every change is published as an event and not stored: a queue is
//! not part of what was said.
//!
//! A message the user has open for editing is held. While the first message is held, the queue
//! sends nothing by itself: a send that would have gone is owed, and goes once the hold is let
//! go, with whatever text the edit left. An edit box renews its hold; one that went away with
//! its window lets it lapse, and the owed send goes then.
//!
//! The first session of a task started from a prompt is made before the task is set up, with
//! the prompt waiting in its queue. While a task is set up, its sessions' queues send nothing
//! by themselves; the setup sends them when it finishes well. That message is also kept in the
//! database until it goes (`setup_prompt`), and is put back in the queue after a restart: the
//! queue itself lives in memory.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::agent::{AgentCommand, AgentEvent, Auth};
use crate::error::{Error, Result};
use crate::github_mentions::{addresses_in, fresh_list};
use crate::paths::Paths;
use crate::sessions::{Live, Told, is_busy_status, send_text, skill_called};
use crate::store::Store;
use crate::terminal::Terminals;
use crate::{Core, lock};

/// A message waiting to be sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedMessage {
    /// What the message is referred to by while it waits.
    pub id: u64,
    /// What it says.
    pub text: String,
    /// Whether Swarmed put it there by itself, when the usage limit that stopped the session
    /// reset, as the user asked it to. Absent from what the user wrote.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub at_reset: bool,
}

/// How long a hold on a waiting message lasts unless it is renewed. An open edit box renews it
/// every few seconds.
pub(crate) const HOLD_FOR: Duration = Duration::from_secs(10);

/// How often a send that was put off for an edit looks again whether the edit is over.
const LOOK_AGAIN: Duration = Duration::from_millis(500);

/// A waiting message the user has open for editing, and when the edit box last said so.
#[derive(Debug, Clone, Copy)]
struct Hold {
    message: u64,
    renewed: Instant,
}

impl Hold {
    fn lasts(&self) -> bool {
        self.renewed.elapsed() < HOLD_FOR
    }
}

/// Every session's queue.
#[derive(Debug, Default)]
pub(crate) struct Queues {
    waiting: Mutex<HashMap<i64, Vec<QueuedMessage>>>,
    /// The waiting message of each session that the user has open for editing.
    holds: Mutex<HashMap<i64, Hold>>,
    /// The sessions whose first message would have been sent but was held: it goes once the
    /// hold is gone.
    owed: Mutex<HashSet<i64>>,
    /// The sessions whose turn the user stopped, which sends their first waiting message the
    /// moment it ends. Also held while a turn's end is dealt with and while a message is sent
    /// from the queue at once, so that the two cannot both send one.
    pressing: Mutex<HashSet<i64>>,
    /// The sessions of each task being set up here, by the task: their queues send nothing by
    /// themselves until the setup is over.
    setting_up: Mutex<HashMap<i64, Vec<i64>>>,
    /// The waiting message of each session that it was made with and waits for its task's
    /// setup, by its id: kept in the database until it goes. Its lock is taken before the
    /// store's and `waiting`'s, never after.
    kept: Mutex<HashMap<i64, u64>>,
    ids: AtomicU64,
}

impl Queues {
    fn list(&self, session: i64) -> Vec<QueuedMessage> {
        lock(&self.waiting)
            .get(&session)
            .cloned()
            .unwrap_or_default()
    }

    /// The addresses of the pull requests and issues the waiting messages of every session
    /// mention, each once.
    pub(crate) fn addresses_waiting(&self) -> Vec<String> {
        let waiting = lock(&self.waiting);
        let mut addresses: Vec<String> = waiting
            .values()
            .flatten()
            .flat_map(|message| addresses_in(&message.text))
            .collect();
        addresses.sort();
        addresses.dedup();
        addresses
    }

    /// Whether a message is waiting to be sent to the session.
    pub(crate) fn is_waiting(&self, session: i64) -> bool {
        let waiting = lock(&self.waiting);
        waiting.get(&session).is_some_and(|queue| !queue.is_empty())
    }

    fn push(&self, session: i64, text: &str) {
        let message = self.message(text);
        lock(&self.waiting)
            .entry(session)
            .or_default()
            .push(message);
    }

    /// Puts a message first in the queue, ahead of those waiting.
    fn push_front(&self, session: i64, text: &str) {
        let message = self.message(text);
        lock(&self.waiting)
            .entry(session)
            .or_default()
            .insert(0, message);
    }

    /// Puts the message that continues a session when the usage limit that stopped it resets
    /// first in its queue: what the user queued meanwhile goes after it.
    pub(crate) fn push_front_at_reset(&self, session: i64, text: &str) {
        let message = QueuedMessage {
            at_reset: true,
            ..self.message(text)
        };
        lock(&self.waiting)
            .entry(session)
            .or_default()
            .insert(0, message);
    }

    fn message(&self, text: &str) -> QueuedMessage {
        let id = self.ids.fetch_add(1, Ordering::Relaxed) + 1;
        QueuedMessage {
            id,
            text: text.to_string(),
            at_reset: false,
        }
    }

    /// Gives a waiting message new text, which makes it the user's own; text with nothing in
    /// it takes the message out.
    fn edit(&self, session: i64, id: u64, text: &str) {
        let mut waiting = lock(&self.waiting);
        let queue = waiting.entry(session).or_default();
        if text.trim().is_empty() {
            queue.retain(|message| message.id != id);
        } else if let Some(message) = queue.iter_mut().find(|message| message.id == id) {
            message.text = text.to_string();
            // The user's own words now, whatever put the message there.
            message.at_reset = false;
        }
    }

    /// Puts a waiting message first, the others keeping their order. `false` if it is no
    /// longer waiting.
    fn to_front(&self, session: i64, id: u64) -> bool {
        let mut waiting = lock(&self.waiting);
        let Some(queue) = waiting.get_mut(&session) else {
            return false;
        };
        let Some(at) = queue.iter().position(|message| message.id == id) else {
            return false;
        };
        queue[..=at].rotate_right(1);
        true
    }

    /// The sessions whose first waiting message goes as soon as their turn ends. Neither a
    /// turn's end nor a message sent at once sends from a queue while this is held.
    pub(crate) fn pressing(&self) -> MutexGuard<'_, HashSet<i64>> {
        lock(&self.pressing)
    }

    /// Holds a waiting message while the user edits it, or renews the hold. `false` if it is no
    /// longer waiting.
    fn hold(&self, session: i64, id: u64) -> bool {
        if !self.list(session).iter().any(|message| message.id == id) {
            return false;
        }
        let hold = Hold {
            message: id,
            renewed: Instant::now(),
        };
        lock(&self.holds).insert(session, hold);
        true
    }

    /// Lets go of the hold on a waiting message, if it has one.
    fn let_go(&self, session: i64, id: u64) {
        let mut holds = lock(&self.holds);
        if holds.get(&session).is_some_and(|hold| hold.message == id) {
            holds.remove(&session);
        }
    }

    /// Whether the session's first waiting message is held, by a hold that has not lapsed.
    fn first_held(&self, session: i64) -> bool {
        let first = self.list(session).first().map(|message| message.id);
        let holds = lock(&self.holds);
        let hold = holds.get(&session).filter(|hold| hold.lasts());
        first.is_some() && hold.map(|hold| hold.message) == first
    }

    /// Notes that the session's first message would have been sent now. `true` if that was
    /// not noted already.
    fn owe(&self, session: i64) -> bool {
        lock(&self.owed).insert(session)
    }

    fn is_owed(&self, session: i64) -> bool {
        lock(&self.owed).contains(&session)
    }

    /// Takes back what was owed to the session. `true` if anything was.
    fn settle(&self, session: i64) -> bool {
        lock(&self.owed).remove(&session)
    }

    /// Takes waiting message `id` out of the queue, if it is still there.
    fn take(&self, session: i64, id: u64) -> Option<QueuedMessage> {
        let mut waiting = lock(&self.waiting);
        let queue = waiting.get_mut(&session)?;
        let at = queue.iter().position(|message| message.id == id)?;
        Some(queue.remove(at))
    }

    fn take_first(&self, session: i64) -> Option<QueuedMessage> {
        let mut waiting = lock(&self.waiting);
        let queue = waiting.get_mut(&session)?;
        (!queue.is_empty()).then(|| queue.remove(0))
    }

    fn put_back(&self, session: i64, message: QueuedMessage) {
        lock(&self.waiting)
            .entry(session)
            .or_default()
            .insert(0, message);
    }

    pub(crate) fn clear(&self, session: i64) {
        lock(&self.waiting).remove(&session);
        lock(&self.holds).remove(&session);
        lock(&self.owed).remove(&session);
        lock(&self.kept).remove(&session);
    }

    /// Holds the queues of `sessions`, those of `task`, while the task is set up.
    pub(crate) fn hold_for_setup(&self, task: i64, sessions: &[i64]) {
        lock(&self.setting_up).insert(task, sessions.to_vec());
    }

    /// Lets go of the queues of `task`'s sessions: its setup is over.
    pub(crate) fn release_for_setup(&self, task: i64) {
        lock(&self.setting_up).remove(&task);
    }

    /// Whether the session's queue waits for its task's setup.
    pub(crate) fn held_for_setup(&self, session: i64) -> bool {
        let setting_up = lock(&self.setting_up);
        setting_up
            .values()
            .any(|sessions| sessions.contains(&session))
    }

    /// The sessions of `task` whose queues wait for its setup.
    pub(crate) fn held_of(&self, task: i64) -> Vec<i64> {
        lock(&self.setting_up)
            .get(&task)
            .cloned()
            .unwrap_or_default()
    }

    /// Adds `session` to those of `task` whose queues wait for its setup.
    fn hold_too(&self, task: i64, session: i64) {
        lock(&self.setting_up)
            .entry(task)
            .or_default()
            .push(session);
    }

    // The message a session keeps for its task's setup is in two places: here, and in the
    // database, which is where it is found again after a restart. Each change to either is
    // made under the lock on `kept`, and so is each look at both: a look made between the
    // two halves of a change found the message in the database and not here, and put it back
    // in the queue after it had gone to the agent.

    /// Puts `text` last in the session's queue as the message it keeps for its task's setup,
    /// and keeps it in the database until it goes.
    fn keep(&self, store: &Mutex<Store>, session: i64, text: &str) -> Result<()> {
        let mut kept = lock(&self.kept);
        lock(store).session_keep_setup_prompt(session, Some(text))?;
        let message = self.message(text);
        kept.insert(session, message.id);
        lock(&self.waiting)
            .entry(session)
            .or_default()
            .push(message);
        Ok(())
    }

    /// Puts the message the database keeps for the session's task's setup first in its queue,
    /// unless it is kept here already. `true` if it was put there.
    fn keep_again(&self, store: &Mutex<Store>, session: i64) -> Result<bool> {
        let mut kept = lock(&self.kept);
        if kept.contains_key(&session) {
            return Ok(false);
        }
        let Some(text) = lock(store).session_setup_prompt(session)? else {
            return Ok(false);
        };
        let message = self.message(&text);
        kept.insert(session, message.id);
        lock(&self.waiting)
            .entry(session)
            .or_default()
            .insert(0, message);
        Ok(true)
    }

    /// Forgets message `id`, taken out of the session's queue, as the one kept for its task's
    /// setup, if it is: in the database, then here. `true` if it was. One the database cannot
    /// forget stays kept here too, so that it is not put back in the queue.
    fn unkeep(&self, store: &Mutex<Store>, session: i64, id: u64) -> Result<bool> {
        let mut kept = lock(&self.kept);
        if kept.get(&session) != Some(&id) {
            return Ok(false);
        }
        lock(store).session_keep_setup_prompt(session, None)?;
        kept.remove(&session);
        Ok(true)
    }

    /// Whether message `id`, taken out of the session's queue, is the one kept for its task's
    /// setup and the database no longer keeps it: another Swarmed has sent it. It is no longer
    /// kept here either then.
    fn kept_went_elsewhere(&self, store: &Mutex<Store>, session: i64, id: u64) -> bool {
        let mut kept = lock(&self.kept);
        if kept.get(&session) != Some(&id) {
            return false;
        }
        // One that cannot be read is taken to be kept still: better sent than lost.
        let gone = matches!(lock(store).session_setup_prompt(session), Ok(None));
        if gone {
            kept.remove(&session);
        }
        gone
    }

    /// Keeps the database in step with an edit of message `id`, if it is the one the session
    /// keeps for its task's setup: its new text, or nothing once text with nothing in it took
    /// it out.
    fn kept_edited(&self, store: &Mutex<Store>, session: i64, id: u64, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return self.unkeep(store, session, id).map(drop);
        }
        let kept = lock(&self.kept);
        if kept.get(&session) == Some(&id) {
            lock(store).session_keep_setup_prompt(session, Some(text))?;
        }
        Ok(())
    }

    /// The id of the message the session keeps for its task's setup, if it is in the queue.
    fn kept_of(&self, session: i64) -> Option<u64> {
        lock(&self.kept).get(&session).copied()
    }
}

/// What sending from a queue needs, owned, so that a send put off for an edit can be made later.
#[derive(Clone)]
pub(crate) struct Outbox {
    pub(crate) store: Arc<Mutex<Store>>,
    pub(crate) live: Arc<Live>,
    pub(crate) terminals: Arc<Terminals>,
    pub(crate) paths: Paths,
}

/// Tells every front end what a session's queue now holds.
pub(crate) fn publish(store: &Mutex<Store>, live: &Live, session: i64) -> Result<()> {
    let messages = live.queues.list(session);
    live.note(store, session, AgentEvent::Queue { messages })
}

/// Puts messages told to the session's agent mid-turn, which it never took in, back at the
/// front of the session's queue in the order they were told. They wait there as any message.
pub(crate) fn wait_again(store: &Mutex<Store>, live: &Live, session: i64, texts: &[String]) {
    if texts.is_empty() {
        return;
    }
    for text in texts.iter().rev() {
        live.queues.push_front(session, text);
    }
    // An error only means the session is gone, and its queue with it.
    publish(store, live, session).ok();
}

/// Sends the session's first waiting message, if it has one. A message that cannot be sent,
/// because the agent is not running, goes back to the front of the queue. One the user has
/// open for editing is not sent: it is owed, and goes once the edit is over.
pub(crate) fn send_next(outbox: &Outbox, session: i64) -> Result<()> {
    let (store, live) = (&*outbox.store, &*outbox.live);
    if live.queues.first_held(session) {
        if live.queues.owe(session) {
            wait_for_edit(outbox.clone(), session);
        }
        return Ok(());
    }
    let Some(message) = live.queues.take_first(session) else {
        return Ok(());
    };
    if live.queues.kept_went_elsewhere(store, session, message.id) {
        return publish(store, live, session);
    }
    // Whatever was owed is being sent now.
    live.queues.settle(session);
    let with = (&*outbox.terminals, &outbox.paths);
    // The pull requests and issues it mentions as they are now, not as when it was typed.
    let text = fresh_list(&message.text, &live.github);
    let sent = send_text(store, live, with, session, (&text, message.at_reset));
    if sent.is_err() {
        live.queues.put_back(session, message);
    } else {
        // An error only means the session is gone, and what it kept with it.
        live.queues.unkeep(store, session, message.id).ok();
    }
    publish(store, live, session)?;
    sent
}

/// Sends the session's first waiting message, as a turn's end does: not while its task is
/// set up, which sends it once it is over.
pub(crate) fn send_next_by_itself(outbox: &Outbox, session: i64) -> Result<()> {
    if outbox.live.queues.held_for_setup(session) {
        return Ok(());
    }
    send_next(outbox, session)
}

/// Looks again every [`LOOK_AGAIN`] whether the edit that holds back an owed send is over, for
/// an edit box that went away without letting go: its hold lapses and the send goes. Without
/// a runtime nothing looks: letting go sends it then.
fn wait_for_edit(outbox: Outbox, session: i64) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    runtime.spawn(async move {
        while outbox.live.queues.is_owed(session) {
            tokio::time::sleep(LOOK_AGAIN).await;
            send_owed(&outbox, session).ok();
        }
    });
}

/// Sends the session's first waiting message if a send was owed and the edit that held it back
/// is over. Not while the agent is busy again: the end of that turn sends it.
fn send_owed(outbox: &Outbox, session: i64) -> Result<()> {
    let queues = &outbox.live.queues;
    // Held until the message has gone: nothing else sends from the queue meanwhile.
    let _pressing = queues.pressing();
    if queues.first_held(session) || !queues.settle(session) {
        return Ok(());
    }
    let busy = lock(&outbox.store)
        .session_by_id(session)
        .is_ok_and(|session| is_busy_status(session.status));
    if busy {
        return Ok(());
    }
    send_next_by_itself(outbox, session)
}

impl Core {
    /// The messages waiting for the session's agent, in the order they will be sent. The
    /// message the session was made with and keeps for its task's setup is among them, also
    /// after a restart.
    pub fn session_queue(&self, id: i64) -> Result<Vec<QueuedMessage>> {
        self.session(id)?;
        self.queue_kept_again(id)?;
        Ok(self.live.queues.list(id))
    }

    /// Puts `text`, the message a new session is made with, in its queue to wait for its
    /// task's setup, and keeps it in the database until it goes.
    pub(crate) fn keep_for_setup(&self, task: i64, id: i64, text: &str) -> Result<()> {
        self.live.queues.keep(&self.store, id, text)?;
        self.live.queues.hold_too(task, id);
        self.github_follow_waiting();
        publish(&self.store, &self.live, id)
    }

    /// Puts the message the session keeps for its task's setup back first in its queue, where
    /// a restart or closing the session took it out. Not while another Swarmed sets the task
    /// up: the message waits in that one's queue.
    pub(crate) fn queue_kept_again(&self, id: i64) -> Result<()> {
        // Looked at first without the lock, since most sessions keep nothing; `keep_again`
        // looks again under it.
        if self.live.queues.kept_of(id).is_some() {
            return Ok(());
        }
        if self.store().session_setup_prompt(id)?.is_none() {
            return Ok(());
        }
        let session = self.session(id)?;
        let task = self.store().task_by_id(session.task_id)?;
        if task.is_some_and(|task| self.setup_runs_elsewhere(&task)) {
            return Ok(());
        }
        if self.live.queues.keep_again(&self.store, id)? {
            self.github_follow_waiting();
            publish(&self.store, &self.live, id)?;
        }
        Ok(())
    }

    /// Adds a message to the end of the session's queue and returns the queue. A command
    /// Swarmed does not send on is refused here as it is when sent: [`Error::TerminalOnly`].
    pub fn session_enqueue(&self, id: i64, text: &str) -> Result<Vec<QueuedMessage>> {
        self.may_wait(id, text)?;
        if text.trim().is_empty() {
            return Err(Error::EmptyMessage);
        }
        self.live.queues.push(id, text);
        self.queued_for(id);
        self.github_follow_waiting();
        self.queue_changed(id)
    }

    /// Gives a waiting message new text and returns the queue. Text with nothing in it takes
    /// the message out; a message that has been sent meanwhile is left alone. A command
    /// Swarmed does not send on is refused, and the message stays as it was. Either way the
    /// edit is over: a hold on the message is let go, and a send owed meanwhile goes now. That
    /// send failing does not undo the edit: the message is back in the queue then.
    pub fn session_queue_edit(
        &self,
        id: i64,
        message: u64,
        text: &str,
    ) -> Result<Vec<QueuedMessage>> {
        let edited = self.may_wait(id, text);
        if edited.is_ok() {
            self.live.queues.edit(id, message, text);
            self.live
                .queues
                .kept_edited(&self.store, id, message, text)?;
            self.github_follow_waiting();
        }
        self.live.queues.let_go(id, message);
        send_owed(&self.outbox(), id).ok();
        edited?;
        self.queue_changed(id)
    }

    /// Holds a waiting message while the user edits it, or renews the hold. While the first
    /// message is held, the queue sends nothing by itself; a hold that is not renewed within ten
    /// seconds lapses. `false` if the message is no longer waiting.
    pub fn session_queue_hold(&self, id: i64, message: u64) -> Result<bool> {
        self.session(id)?;
        Ok(self.live.queues.hold(id, message))
    }

    /// Lets go of a waiting message the user had open for editing and left as it was. A send
    /// owed meanwhile goes now, with the message's text as it is.
    pub fn session_queue_let_go(&self, id: i64, message: u64) -> Result<()> {
        self.session(id)?;
        self.live.queues.let_go(id, message);
        send_owed(&self.outbox(), id)
    }

    /// Sends the first waiting message now, instead of when a turn ends. For a queue left
    /// standing by a turn that failed or an agent that was stopped. A message the user has
    /// open for editing waits until the edit is over.
    pub fn session_send_queued(&self, id: i64) -> Result<()> {
        send_next(&self.outbox(), id)
    }

    /// Sends a waiting message now, ahead of the others. A turn under way is stopped first and
    /// the message goes the moment that turn has ended, however it ends; an agent that is not
    /// running is started. A message that has been sent meanwhile is left alone. Must be
    /// called from within a tokio runtime.
    pub fn session_send_now(&self, id: i64, message: u64, auth: Auth) -> Result<()> {
        let waits = |queue: Vec<QueuedMessage>| queue.iter().any(|queued| queued.id == message);
        if !waits(self.live.queues.list(id)) {
            return Ok(());
        }
        // A session can count as busy with no agent running, left so by a Swarmed that ended
        // in the middle of a turn: there is no turn to stop then.
        let running = self.live.is_running(id);
        self.session_start(id, auth)?;
        // Held from putting it first to sending it: a turn that ends meanwhile sends it then.
        let pressing = self.live.queues.pressing();
        if !self.live.queues.to_front(id, message) {
            return Ok(());
        }
        self.send_first_now(id, running, pressing)
    }

    /// Sends `text` now, ahead of the messages waiting, as [`Core::session_send_now`] does for
    /// one of them: in one step, so that no turn can end between its being put first and its
    /// being sent. A command Swarmed does not send on is refused, as when it is queued. Must be
    /// called from within a tokio runtime.
    pub fn session_send_text_now(&self, id: i64, text: &str, auth: Auth) -> Result<()> {
        self.may_wait(id, text)?;
        if text.trim().is_empty() {
            return Err(Error::EmptyMessage);
        }
        let running = self.live.is_running(id);
        self.session_start(id, auth)?;
        let pressing = self.live.queues.pressing();
        self.live.queues.push_front(id, text);
        self.send_first_now(id, running, pressing)
    }

    /// Sends the session's first waiting message now: if a turn is under way, it is stopped and
    /// the message goes when it has ended (marked in `pressing`, held from before the message
    /// was put first); else at once. `running` is whether the agent ran before this.
    fn send_first_now(
        &self,
        id: i64,
        running: bool,
        mut pressing: MutexGuard<'_, HashSet<i64>>,
    ) -> Result<()> {
        publish(&self.store, &self.live, id)?;
        let busy = self
            .session(id)
            .is_ok_and(|session| is_busy_status(session.status));
        if running && busy {
            self.live.command(id, AgentCommand::Interrupt)?;
            pressing.insert(id);
            return Ok(());
        }
        send_next(&self.outbox(), id)
    }

    /// Tells a waiting message to the session's agent now, in the middle of its turn, without
    /// stopping it: for an agent that takes a message into the turn under way, at its next
    /// step (`AgentInfo::hears_mid_turn`). It is recorded where the agent says it heard it,
    /// and an open request stays open. An agent between turns gets it as any message. A
    /// message no longer waiting is left alone.
    ///
    /// As tried with Claude Code 2.1.330: a `user` line written mid-turn is taken in after the
    /// tool under way, answered in the same turn, and an open `can_use_tool` request stays open.
    pub fn session_tell_now(&self, id: i64, message: u64) -> Result<()> {
        let session = self.session(id)?;
        if !self.agents_now().hears_mid_turn(&session.agent) {
            return Err(Error::HearsNoMessageMidTurn(session.agent));
        }
        // Held throughout: no turn's end sends from the queue meanwhile.
        let _pressing = self.live.queues.pressing();
        let busy = self.live.is_running(id)
            && self
                .session(id)
                .is_ok_and(|session| is_busy_status(session.status));
        if !busy {
            return self.send_between_turns(id, message);
        }
        let Some(taken) = self.live.queues.take(id, message) else {
            return Ok(());
        };
        // The pull requests and issues it mentions as they are now, as when it is sent.
        let text = fresh_list(&taken.text, &self.live.github);
        // A skill the agent would not know by name is told as what it says, like any message
        // that calls one; what is recorded is what was typed.
        let called = skill_called(&self.store, &self.paths, id, &text);
        let message = Told {
            sent: called.unwrap_or_else(|| text.clone()),
            typed: text,
        };
        let told = self.live.tell(id, message);
        if told.is_err() {
            self.live.queues.put_back(id, taken);
        } else {
            self.live.queues.unkeep(&self.store, id, taken.id)?;
        }
        publish(&self.store, &self.live, id)?;
        told
    }

    /// Sends a waiting message to an agent between turns, as any message is sent: put first,
    /// then sent. Nothing if it is no longer waiting.
    fn send_between_turns(&self, id: i64, message: u64) -> Result<()> {
        if !self.live.queues.to_front(id, message) {
            return Ok(());
        }
        send_next(&self.outbox(), id)
    }

    /// What sending from a queue needs, for one that may be put off.
    pub(crate) fn outbox(&self) -> Outbox {
        Outbox {
            store: Arc::clone(&self.store),
            live: Arc::clone(&self.live),
            terminals: Arc::clone(&self.terminals),
            paths: self.paths.clone(),
        }
    }

    fn queue_changed(&self, id: i64) -> Result<Vec<QueuedMessage>> {
        publish(&self.store, &self.live, id)?;
        Ok(self.live.queues.list(id))
    }

    /// Sends the first waiting message now, starting the session's agent first if it is not
    /// running: a queue can be left standing by an agent that has since been stopped. Must be
    /// called from within a tokio runtime.
    pub fn session_resume_queue(&self, id: i64, auth: Auth) -> Result<()> {
        self.queue_kept_again(id)?;
        self.session_start(id, auth)?;
        self.session_send_queued(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_sent_at_a_reset_is_the_users_own_once_they_change_it() {
        let queues = Queues::default();
        queues.push_front_at_reset(1, "Continue where you stopped.");
        let waiting = queues.list(1);
        assert!(waiting[0].at_reset);
        queues.edit(1, waiting[0].id, "Continue, but leave the tests alone.");
        let waiting = queues.list(1);
        assert_eq!(waiting[0].text, "Continue, but leave the tests alone.");
        assert!(!waiting[0].at_reset);
    }

    #[test]
    fn a_hold_that_is_not_renewed_lapses() {
        let queues = Queues::default();
        queues.push(1, "first");
        let id = queues.list(1)[0].id;
        assert!(queues.hold(1, id));
        assert!(queues.first_held(1));
        let long_ago = Instant::now()
            .checked_sub(HOLD_FOR + Duration::from_secs(1))
            .expect("the clock goes back that far");
        lock(&queues.holds).insert(
            1,
            Hold {
                message: id,
                renewed: long_ago,
            },
        );
        assert!(!queues.first_held(1));
        // Renewed, it holds again; let go, it does not.
        assert!(queues.hold(1, id));
        assert!(queues.first_held(1));
        queues.let_go(1, id);
        assert!(!queues.first_held(1));
        assert!(!queues.hold(1, id + 1));
    }
}

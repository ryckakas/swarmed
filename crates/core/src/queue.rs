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

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::agent::{AgentCommand, AgentEvent, Auth};
use crate::error::{Error, Result};
use crate::paths::Paths;
use crate::sessions::{Live, is_busy_status, send_text};
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
    ids: AtomicU64,
}

impl Queues {
    fn list(&self, session: i64) -> Vec<QueuedMessage> {
        lock(&self.waiting)
            .get(&session)
            .cloned()
            .unwrap_or_default()
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

    fn message(&self, text: &str) -> QueuedMessage {
        let id = self.ids.fetch_add(1, Ordering::Relaxed) + 1;
        QueuedMessage {
            id,
            text: text.to_string(),
        }
    }

    /// Gives a waiting message new text; text with nothing in it takes the message out.
    fn edit(&self, session: i64, id: u64, text: &str) {
        let mut waiting = lock(&self.waiting);
        let queue = waiting.entry(session).or_default();
        if text.trim().is_empty() {
            queue.retain(|message| message.id != id);
        } else if let Some(message) = queue.iter_mut().find(|message| message.id == id) {
            message.text = text.to_string();
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

    /// The sessions whose first waiting message goes as soon as their turn ends. Nothing else
    /// sends from a queue while this is held.
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
fn publish(store: &Mutex<Store>, live: &Live, session: i64) -> Result<()> {
    let messages = live.queues.list(session);
    live.note(store, session, AgentEvent::Queue { messages })
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
    // Whatever was owed is being sent now.
    live.queues.settle(session);
    let with = (&*outbox.terminals, &outbox.paths);
    let sent = send_text(store, live, with, session, &message.text);
    if sent.is_err() {
        live.queues.put_back(session, message);
    }
    publish(store, live, session)?;
    sent
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
    send_next(outbox, session)
}

impl Core {
    /// The messages waiting for the session's agent, in the order they will be sent.
    pub fn session_queue(&self, id: i64) -> Result<Vec<QueuedMessage>> {
        self.session(id)?;
        Ok(self.live.queues.list(id))
    }

    /// Adds a message to the end of the session's queue and returns the queue. A command
    /// Swarmed does not send on is refused here as it is when sent: [`Error::TerminalOnly`].
    pub fn session_enqueue(&self, id: i64, text: &str) -> Result<Vec<QueuedMessage>> {
        self.may_wait(id, text)?;
        if text.trim().is_empty() {
            return Err(Error::EmptyMessage);
        }
        self.live.queues.push(id, text);
        self.queue_changed(id)
    }

    /// Gives a waiting message new text and returns the queue. Text with nothing in it takes
    /// the message out; a message that has been sent meanwhile is left alone. A command
    /// Swarmed does not send on is refused, and the message stays as it was. Either way the
    /// edit is over: a hold on the message is let go, and a send owed meanwhile goes now.
    pub fn session_queue_edit(
        &self,
        id: i64,
        message: u64,
        text: &str,
    ) -> Result<Vec<QueuedMessage>> {
        let edited = self.may_wait(id, text);
        if edited.is_ok() {
            self.live.queues.edit(id, message, text);
        }
        self.live.queues.let_go(id, message);
        send_owed(&self.outbox(), id)?;
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
            pressing.insert(id);
            return self.live.command(id, AgentCommand::Interrupt);
        }
        send_next(&self.outbox(), id)
    }

    /// Tells a waiting message to the session's agent now, in the middle of its turn, without
    /// stopping it: for an agent that takes a message into the turn under way, at its next
    /// step (`AgentInfo::hears_mid_turn`). It is recorded where the agent says it heard it,
    /// and an open request stays open. An agent between turns gets it as any message. A
    /// message no longer waiting is left alone.
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
        let told = self.live.tell(id, &taken.text);
        if told.is_err() {
            self.live.queues.put_back(id, taken);
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
        self.session_start(id, auth)?;
        self.session_send_queued(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

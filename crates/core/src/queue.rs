//! Messages the user wrote while an agent was busy, waiting for its turn to end.
//!
//! A queue belongs to a session and lives only as long as the core does. When a turn ends well,
//! or the user stops it, the first message is sent as if the user had just typed it; a turn
//! that failed leaves the queue as it is, for the user to decide, and so does an agent that
//! was stopped. The user can also send a waiting message at once, which puts it first and
//! stops the turn under way. Every change is published as an event and not stored: a queue is
//! not part of what was said.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

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

/// Every session's queue.
#[derive(Debug, Default)]
pub(crate) struct Queues {
    waiting: Mutex<HashMap<i64, Vec<QueuedMessage>>>,
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
        let id = self.ids.fetch_add(1, Ordering::Relaxed) + 1;
        let text = text.to_string();
        lock(&self.waiting)
            .entry(session)
            .or_default()
            .push(QueuedMessage { id, text });
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

    /// Whether `id` is the session's first waiting message.
    fn is_first(&self, session: i64, id: u64) -> bool {
        lock(&self.waiting)
            .get(&session)
            .and_then(|queue| queue.first())
            .is_some_and(|first| first.id == id)
    }

    /// The sessions whose first waiting message goes as soon as their turn ends. Neither a
    /// turn's end nor a message sent at once sends from a queue while this is held.
    pub(crate) fn pressing(&self) -> MutexGuard<'_, HashSet<i64>> {
        lock(&self.pressing)
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
    }
}

/// Tells every front end what a session's queue now holds.
fn publish(store: &Mutex<Store>, live: &Live, session: i64) -> Result<()> {
    let messages = live.queues.list(session);
    live.note(store, session, AgentEvent::Queue { messages })
}

/// Sends the session's first waiting message, if it has one. A message that cannot be sent,
/// because the agent is not running, goes back to the front of the queue.
pub(crate) fn send_next(
    store: &Mutex<Store>,
    live: &Live,
    with: (&Terminals, &Paths),
    session: i64,
) -> Result<()> {
    let Some(message) = live.queues.take_first(session) else {
        return Ok(());
    };
    let sent = send_text(store, live, with, session, &message.text);
    if sent.is_err() {
        live.queues.put_back(session, message);
    }
    publish(store, live, session)?;
    sent
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
    /// Swarmed does not send on is refused, and the message stays as it was.
    pub fn session_queue_edit(
        &self,
        id: i64,
        message: u64,
        text: &str,
    ) -> Result<Vec<QueuedMessage>> {
        self.may_wait(id, text)?;
        self.live.queues.edit(id, message, text);
        self.queue_changed(id)
    }

    /// Sends the first waiting message now, instead of when a turn ends. For a queue left
    /// standing by a turn that failed or an agent that was stopped.
    pub fn session_send_queued(&self, id: i64) -> Result<()> {
        send_next(&self.store, &self.live, (&*self.terminals, &self.paths), id)
    }

    /// Sends a waiting message now, ahead of the others. A turn under way is stopped first and
    /// the message goes the moment that turn has ended, however it ends; an agent that is not
    /// running is started. A message that has been sent meanwhile is left alone. Must be
    /// called from within a tokio runtime.
    pub fn session_send_now(&self, id: i64, message: u64, auth: Auth) -> Result<()> {
        if !self.live.queues.to_front(id, message) {
            return Ok(());
        }
        publish(&self.store, &self.live, id)?;
        // A session can count as busy with no agent running, left so by a Swarmed that ended
        // in the middle of a turn: there is no turn to stop then.
        let running = self.live.is_running(id);
        self.session_start(id, auth)?;
        let mut pressing = self.live.queues.pressing();
        // The turn may have ended since the message was put first, and the end sent it: the
        // turn it started is then the one the user asked for, and is not to be stopped.
        if !self.live.queues.is_first(id, message) {
            return Ok(());
        }
        let busy = self
            .session(id)
            .is_ok_and(|session| is_busy_status(session.status));
        if running && busy {
            self.live.command(id, AgentCommand::Interrupt)?;
            pressing.insert(id);
            return Ok(());
        }
        send_next(&self.store, &self.live, (&*self.terminals, &self.paths), id)
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

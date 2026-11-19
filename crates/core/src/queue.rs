//! Messages the user wrote while an agent was busy, waiting for its turn to end.
//!
//! A queue belongs to a session and lives only as long as the core does. When a turn ends well
//! the first message is sent as if the user had just typed it; a turn that was stopped or
//! failed leaves the queue as it is, for the user to decide. Every change is published as an
//! event and not stored: a queue is not part of what was said.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::agent::AgentEvent;
use crate::error::{Error, Result};
use crate::sessions::{Live, send_text};
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
    ids: AtomicU64,
}

impl Queues {
    fn list(&self, session: i64) -> Vec<QueuedMessage> {
        lock(&self.waiting)
            .get(&session)
            .cloned()
            .unwrap_or_default()
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
    terminals: &Terminals,
    session: i64,
) -> Result<()> {
    let Some(message) = live.queues.take_first(session) else {
        return Ok(());
    };
    let sent = send_text(store, live, terminals, session, &message.text);
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

    /// Adds a message to the end of the session's queue and returns the queue.
    pub fn session_enqueue(&self, id: i64, text: &str) -> Result<Vec<QueuedMessage>> {
        self.session(id)?;
        if text.trim().is_empty() {
            return Err(Error::EmptyMessage);
        }
        self.live.queues.push(id, text);
        self.queue_changed(id)
    }

    /// Gives a waiting message new text and returns the queue. Text with nothing in it takes
    /// the message out; a message that has been sent meanwhile is left alone.
    pub fn session_queue_edit(
        &self,
        id: i64,
        message: u64,
        text: &str,
    ) -> Result<Vec<QueuedMessage>> {
        self.session(id)?;
        self.live.queues.edit(id, message, text);
        self.queue_changed(id)
    }

    /// Sends the first waiting message now, instead of when a turn ends. For a queue left
    /// standing by a turn that was stopped or failed.
    pub fn session_send_queued(&self, id: i64) -> Result<()> {
        send_next(&self.store, &self.live, &self.terminals, id)
    }

    fn queue_changed(&self, id: i64) -> Result<Vec<QueuedMessage>> {
        publish(&self.store, &self.live, id)?;
        Ok(self.live.queues.list(id))
    }
}

//! A connection to Chrome over its DevTools protocol: commands that are answered, and events.
//!
//! Every command carries a number, and Chrome answers it with the same number. Events carry
//! none. Commands for one page are sent on that page's session, the `sessionId` Chrome gave
//! when Swarmed attached to it, all over the one connection (`flatten`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use super::ws::{self, Message};
use crate::lock;

/// How long a command may wait for its answer. Navigating answers once the new page is
/// on its way, not when it has loaded.
const ANSWER_LIMIT: Duration = Duration::from_secs(30);

/// How long Chrome has to take a connection: it is on this machine, and answers at once.
const CONNECT_LIMIT: Duration = Duration::from_secs(10);

/// What became of a command: its result, or why there is none.
type Answer = Result<Value, String>;

/// Something Chrome said without being asked.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Event {
    /// The page's session it came on, or `None` for the browser as a whole.
    pub(crate) session: Option<String>,
    pub(crate) method: String,
    pub(crate) params: Value,
}

/// One connection to one Chrome.
#[derive(Debug)]
pub(crate) struct Cdp {
    next: AtomicU64,
    /// Whether the connection still stands: Chrome has not closed it, nor has Swarmed.
    open: AtomicBool,
    waiting: Mutex<HashMap<u64, oneshot::Sender<Answer>>>,
    out: mpsc::UnboundedSender<Out>,
}

/// What the writer is to send.
enum Out {
    Text(String),
    Pong(Vec<u8>),
    /// Swarmed is done with the connection.
    Close,
}

impl Cdp {
    /// Connects to the endpoint Chrome wrote down. Its events arrive on the receiver, which
    /// ends when the connection does.
    pub(crate) async fn connect(
        port: u16,
        path: &str,
    ) -> std::io::Result<(Arc<Self>, mpsc::UnboundedReceiver<Event>)> {
        let (reader, writer) = ws::connect(port, path, CONNECT_LIMIT).await?;
        let (out, outgoing) = mpsc::unbounded_channel();
        let (events, heard) = mpsc::unbounded_channel();
        let cdp = Arc::new(Self {
            next: AtomicU64::new(1),
            open: AtomicBool::new(true),
            waiting: Mutex::default(),
            out,
        });
        drop(tokio::spawn(write(writer, outgoing)));
        drop(tokio::spawn(read(reader, Arc::clone(&cdp), events)));
        Ok((cdp, heard))
    }

    /// Sends `method` with `params`, on `session` if one is given, and waits for the answer.
    /// What Chrome refuses comes back as its own words.
    pub(crate) async fn call(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let message = message(id, session, method, params);
        let (answer, answered) = oneshot::channel();
        lock(&self.waiting).insert(id, answer);
        if !self.is_open() || self.out.send(Out::Text(message)).is_err() {
            lock(&self.waiting).remove(&id);
            return Err(GONE.to_string());
        }
        match tokio::time::timeout(ANSWER_LIMIT, answered).await {
            Ok(Ok(answer)) => answer,
            Ok(Err(_)) => Err(GONE.to_string()),
            Err(_) => {
                lock(&self.waiting).remove(&id);
                Err(format!("Chrome did not answer {method} in time"))
            }
        }
    }

    /// Sends `method` without waiting for the answer, for what nobody waits on.
    pub(crate) fn send(&self, session: Option<&str>, method: &str, params: Value) {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let message = message(id, session, method, params);
        self.out.send(Out::Text(message)).ok();
    }

    /// Whether the connection still stands.
    pub(crate) fn is_open(&self) -> bool {
        self.open.load(Ordering::Relaxed) && !self.out.is_closed()
    }

    /// Ends the connection. Chrome goes on running.
    pub(crate) fn close(&self) {
        self.open.store(false, Ordering::Relaxed);
        self.out.send(Out::Close).ok();
    }

    /// Hands an answer to whoever waits for it.
    fn answered(&self, id: u64, answer: Answer) {
        if let Some(waiting) = lock(&self.waiting).remove(&id) {
            waiting.send(answer).ok();
        }
    }

    /// Fails every command still waiting: no answer will come.
    fn ended(&self) {
        self.open.store(false, Ordering::Relaxed);
        for (_, waiting) in lock(&self.waiting).drain() {
            waiting.send(Err(GONE.to_string())).ok();
        }
    }
}

/// Command `id`, as Chrome takes it: `method` with `params`, on `session` if one is given.
fn message(id: u64, session: Option<&str>, method: &str, params: Value) -> String {
    let mut message = serde_json::Map::new();
    message.insert("id".into(), id.into());
    message.insert("method".into(), method.into());
    message.insert("params".into(), params);
    if let Some(session) = session {
        message.insert("sessionId".into(), session.into());
    }
    Value::Object(message).to_string()
}

/// What a command that cannot be answered any more says.
const GONE: &str = "the connection to Chrome is gone";

async fn write(mut writer: ws::Writer, mut outgoing: mpsc::UnboundedReceiver<Out>) {
    while let Some(out) = outgoing.recv().await {
        let sent = match out {
            Out::Text(text) => writer.text(&text).await,
            Out::Pong(payload) => writer.pong(&payload).await,
            Out::Close => break,
        };
        if sent.is_err() {
            return;
        }
    }
    writer.close().await;
}

async fn read(mut reader: ws::Reader, cdp: Arc<Cdp>, events: mpsc::UnboundedSender<Event>) {
    while let Ok(Some(message)) = reader.next().await {
        match message {
            Message::Ping(payload) => {
                cdp.out.send(Out::Pong(payload)).ok();
            }
            Message::Text(text) => take(&cdp, &events, &text),
        }
    }
    cdp.ended();
    // The writer goes with the last sender; the events' receiver hears the end.
}

/// Hands one message on: an answer to whoever asked, an event to the events.
fn take(cdp: &Cdp, events: &mpsc::UnboundedSender<Event>, text: &str) {
    let Ok(message) = serde_json::from_str::<Value>(text) else {
        return;
    };
    match parse(message) {
        Some(Parsed::Answer(id, answer)) => cdp.answered(id, answer),
        Some(Parsed::Event(event)) => {
            events.send(event).ok();
        }
        None => {}
    }
}

/// A message from Chrome, as what it is.
#[derive(Debug, PartialEq)]
enum Parsed {
    Answer(u64, Answer),
    Event(Event),
}

fn parse(mut message: Value) -> Option<Parsed> {
    if let Some(id) = message.get("id").and_then(Value::as_u64) {
        let answer = match message.get_mut("error") {
            Some(error) => Err(error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Chrome refused it")
                .to_string()),
            None => Ok(message.get_mut("result").map_or_default(Value::take)),
        };
        return Some(Parsed::Answer(id, answer));
    }
    let method = message.get("method")?.as_str()?.to_string();
    Some(Parsed::Event(Event {
        session: message
            .get("sessionId")
            .and_then(Value::as_str)
            .map(String::from),
        method,
        params: message.get_mut("params").map_or_default(Value::take),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_answer_is_told_from_an_event_and_a_refusal_keeps_chromes_words() {
        let answer = json!({ "id": 7, "result": { "frameId": "F" } });
        assert_eq!(
            parse(answer),
            Some(Parsed::Answer(7, Ok(json!({ "frameId": "F" }))))
        );
        let refused = json!({ "id": 8, "error": { "code": -32000, "message": "No target" } });
        assert_eq!(
            parse(refused),
            Some(Parsed::Answer(8, Err("No target".into())))
        );
        let event = json!({ "method": "Page.loadEventFired", "params": {}, "sessionId": "S" });
        assert_eq!(
            parse(event),
            Some(Parsed::Event(Event {
                session: Some("S".into()),
                method: "Page.loadEventFired".into(),
                params: json!({}),
            }))
        );
        assert_eq!(parse(json!({ "something": "else" })), None);
    }
}

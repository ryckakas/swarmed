//! Agent sessions: creating them, running them, and keeping what they said.

use std::sync::{Arc, Mutex};

use rusqlite::{OptionalExtension, Row, params};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::agent::{AgentCommand, AgentEvent, Auth, SessionSpec, Supervision};
use crate::error::{Error, Result};
use crate::store::Store;
use crate::{Core, lock};

/// How long a session title derived from the first message may be.
const TITLE_CHARS: usize = 60;

/// What a session is doing, as far as the last recorded event tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    /// Waiting for the next message.
    Idle,
    /// A turn is running.
    Working,
    /// A permission request is waiting for a decision.
    Waiting,
    /// The agent process ended with an error.
    Errored,
}

impl SessionStatus {
    /// The value stored in the database and shown to the user.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Waiting => "waiting",
            Self::Errored => "errored",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "working" => Self::Working,
            "waiting" => Self::Waiting,
            "errored" => Self::Errored,
            _ => Self::Idle,
        }
    }

    /// The status an event leads to, or `None` if it changes nothing.
    fn after(event: &AgentEvent) -> Option<Self> {
        match event {
            AgentEvent::UserMessage { .. }
            | AgentEvent::Text { .. }
            | AgentEvent::ToolUse { .. }
            | AgentEvent::ToolResult { .. } => Some(Self::Working),
            AgentEvent::PermissionRequest { .. } => Some(Self::Waiting),
            AgentEvent::TurnDone { .. }
            | AgentEvent::Exited {
                code: None | Some(0),
                ..
            } => Some(Self::Idle),
            AgentEvent::Exited { .. } => Some(Self::Errored),
            AgentEvent::Init { .. } | AgentEvent::RateLimit { .. } | AgentEvent::Other { .. } => {
                None
            }
        }
    }
}

/// One conversation with an agent, in a task's worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The database id.
    pub id: i64,
    /// The task whose worktree the session runs in.
    pub task_id: i64,
    /// A short label; taken from the first message unless one was given.
    pub title: String,
    /// The agent's own id for the conversation, known once it has started.
    pub agent_session_id: Option<String>,
    /// The model the session was created with; `None` is the agent's default.
    pub model: Option<String>,
    /// How much the agent may do without asking.
    pub supervision: Supervision,
    /// What the session is doing.
    pub status: SessionStatus,
}

/// Options for [`Core::session_new`].
#[derive(Debug, Clone, Default)]
pub struct SessionOptions {
    /// A label; without one, the first message is used.
    pub title: Option<String>,
    /// The model to run on; `None` leaves the agent's default.
    pub model: Option<String>,
    /// How much the agent may do without asking.
    pub supervision: Supervision,
}

const SESSION_SELECT: &str =
    "SELECT id, task_id, title, agent_session_id, model, supervision, status FROM sessions";

fn session_from_row(row: &Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        task_id: row.get(1)?,
        title: row.get(2)?,
        agent_session_id: row.get(3)?,
        model: row.get(4)?,
        supervision: Supervision::parse(&row.get::<_, String>(5)?).unwrap_or_default(),
        status: SessionStatus::parse(&row.get::<_, String>(6)?),
    })
}

impl Store {
    fn session_insert(&self, task_id: i64, options: &SessionOptions) -> Result<Session> {
        let title = options.title.clone().unwrap_or_default();
        self.conn.execute(
            "INSERT INTO sessions (task_id, title, model, supervision) VALUES (?1, ?2, ?3, ?4)",
            params![task_id, title, options.model, options.supervision.as_str()],
        )?;
        Ok(Session {
            id: self.conn.last_insert_rowid(),
            task_id,
            title,
            agent_session_id: None,
            model: options.model.clone(),
            supervision: options.supervision,
            status: SessionStatus::Idle,
        })
    }

    fn session_by_id(&self, id: i64) -> Result<Session> {
        let sql = format!("{SESSION_SELECT} WHERE id = ?1");
        let session = self
            .conn
            .query_row(&sql, params![id], session_from_row)
            .optional()?;
        session.ok_or(Error::SessionNotFound(id))
    }

    fn sessions(&self, task_id: i64) -> Result<Vec<Session>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{SESSION_SELECT} WHERE task_id = ?1 ORDER BY id"))?;
        let rows = stmt.query_map(params![task_id], session_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn events(&self, session_id: i64) -> Result<Vec<AgentEvent>> {
        let mut stmt = self
            .conn
            .prepare("SELECT event FROM session_events WHERE session_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map(params![session_id], |row| row.get::<_, String>(0))?;
        let texts = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(texts.iter().map(|text| event_from_json(text)).collect())
    }

    /// Stores an event and applies what it means for the session's row.
    fn record(&self, session_id: i64, event: &AgentEvent) -> Result<()> {
        self.conn.execute(
            "INSERT INTO session_events (session_id, event) VALUES (?1, ?2)",
            params![session_id, serde_json::to_string(event)?],
        )?;
        if let Some(status) = SessionStatus::after(event) {
            self.conn.execute(
                "UPDATE sessions SET status = ?2 WHERE id = ?1",
                params![session_id, status.as_str()],
            )?;
        }
        match event {
            AgentEvent::Init {
                agent_session_id, ..
            } => self.conn.execute(
                "UPDATE sessions SET agent_session_id = ?2 WHERE id = ?1",
                params![session_id, agent_session_id],
            )?,
            AgentEvent::UserMessage { text } => self.conn.execute(
                "UPDATE sessions SET title = ?2 WHERE id = ?1 AND title = ''",
                params![session_id, title_from(text)],
            )?,
            _ => 0,
        };
        Ok(())
    }
}

/// An event written by a newer version may not parse; it is shown as [`AgentEvent::Other`].
fn event_from_json(text: &str) -> AgentEvent {
    serde_json::from_str(text).unwrap_or_else(|_| AgentEvent::Other {
        raw: serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string())),
    })
}

fn title_from(message: &str) -> String {
    let line = message.lines().next().unwrap_or_default().trim();
    if line.chars().count() <= TITLE_CHARS {
        return line.to_string();
    }
    let mut title: String = line.chars().take(TITLE_CHARS - 1).collect();
    title.push('…');
    title
}

impl Core {
    /// Creates a session in a task. Nothing runs until [`Core::session_open`].
    pub fn session_new(
        &self,
        project: &str,
        task: &str,
        options: &SessionOptions,
    ) -> Result<Session> {
        let (_, task) = self.task(project, task)?;
        self.store().session_insert(task.id, options)
    }

    /// A task's sessions, oldest first.
    pub fn sessions(&self, project: &str, task: &str) -> Result<Vec<Session>> {
        let (_, task) = self.task(project, task)?;
        self.store().sessions(task.id)
    }

    /// The session with this id.
    pub fn session(&self, id: i64) -> Result<Session> {
        self.store().session_by_id(id)
    }

    /// Everything a session has said and been told, oldest first.
    pub fn session_events(&self, id: i64) -> Result<Vec<AgentEvent>> {
        self.store().session_by_id(id)?;
        self.store().events(id)
    }

    /// Starts the agent for a session, continuing its conversation if it has one.
    ///
    /// Every event is stored before it is handed out, so history survives a front end that
    /// stops listening. Must be called from within a tokio runtime.
    pub fn session_open(&self, id: i64, auth: Auth) -> Result<LiveSession> {
        let session = self.session(id)?;
        let task = self
            .store()
            .task_by_id(session.task_id)?
            .ok_or(Error::SessionNotFound(id))?;
        let handle = self.backend.start(SessionSpec {
            cwd: task.worktree_path,
            model: session.model,
            supervision: session.supervision,
            auth,
            resume: session.agent_session_id,
            ..SessionSpec::default()
        })?;
        let (event_tx, events) = mpsc::unbounded_channel();
        drop(tokio::spawn(record_and_forward(
            handle.events,
            event_tx,
            Arc::clone(&self.store),
            id,
        )));
        Ok(LiveSession {
            id,
            events,
            commands: handle.commands,
            store: Arc::clone(&self.store),
        })
    }
}

async fn record_and_forward(
    mut from: mpsc::UnboundedReceiver<AgentEvent>,
    to: mpsc::UnboundedSender<AgentEvent>,
    store: Arc<Mutex<Store>>,
    session_id: i64,
) {
    while let Some(event) = from.recv().await {
        // A full disk must not cut the user off from a running agent, so a failed write is
        // skipped and the event still reaches the front end.
        lock(&store).record(session_id, &event).ok();
        to.send(event).ok();
    }
}

/// A session whose agent is running. Dropping it stops the agent.
#[derive(Debug)]
pub struct LiveSession {
    id: i64,
    events: mpsc::UnboundedReceiver<AgentEvent>,
    commands: mpsc::UnboundedSender<AgentCommand>,
    store: Arc<Mutex<Store>>,
}

impl LiveSession {
    /// The session's id.
    #[must_use]
    pub fn id(&self) -> i64 {
        self.id
    }

    /// The next event, or `None` once the agent has exited.
    pub async fn next_event(&mut self) -> Option<AgentEvent> {
        self.events.recv().await
    }

    /// Starts a turn with `text`.
    pub fn send(&self, text: &str) -> Result<()> {
        lock(&self.store).record(
            self.id,
            &AgentEvent::UserMessage {
                text: text.to_string(),
            },
        )?;
        self.command(AgentCommand::UserMessage(text.to_string()))
    }

    /// Lets a pending permission request go ahead.
    pub fn allow(&self, request_id: &str) -> Result<()> {
        self.command(AgentCommand::Allow {
            request_id: request_id.to_string(),
        })
    }

    /// Refuses a pending permission request; the agent is shown `reason`.
    pub fn deny(&self, request_id: &str, reason: &str) -> Result<()> {
        let (request_id, reason) = (request_id.to_string(), reason.to_string());
        self.command(AgentCommand::Deny { request_id, reason })
    }

    /// Stops the current turn. The session stays usable.
    pub fn interrupt(&self) -> Result<()> {
        self.command(AgentCommand::Interrupt)
    }

    fn command(&self, command: AgentCommand) -> Result<()> {
        self.commands
            .send(command)
            .map_err(|_| Error::SessionClosed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_is_the_first_line_shortened() {
        assert_eq!(title_from("  fix the login\nmore detail"), "fix the login");
        let long = "x".repeat(200);
        let title = title_from(&long);
        assert_eq!(title.chars().count(), TITLE_CHARS);
        assert!(title.ends_with('…'));
    }

    #[test]
    fn exit_code_decides_between_idle_and_errored() {
        let exited = |code| AgentEvent::Exited {
            code,
            stderr: String::new(),
        };
        assert_eq!(
            SessionStatus::after(&exited(Some(0))),
            Some(SessionStatus::Idle)
        );
        assert_eq!(
            SessionStatus::after(&exited(None)),
            Some(SessionStatus::Idle)
        );
        assert_eq!(
            SessionStatus::after(&exited(Some(1))),
            Some(SessionStatus::Errored)
        );
    }

    #[test]
    fn unreadable_stored_events_are_kept_as_other() {
        assert!(matches!(
            event_from_json(r#"{"type":"from_the_future"}"#),
            AgentEvent::Other { .. }
        ));
        assert!(matches!(
            event_from_json("not json"),
            AgentEvent::Other { .. }
        ));
    }
}

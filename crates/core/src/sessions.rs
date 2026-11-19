//! Agent sessions: creating them, running them, and keeping what they said.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{broadcast, mpsc};

use crate::agent::{AgentCommand, AgentEvent, Auth, SessionSpec, Supervision};
use crate::agents::CLAUDE;
use crate::error::{Error, Result};
use crate::mcp::{self, McpServer};
use crate::store::{Store, Task};
use crate::{Core, lock};

/// How long a session title derived from the first message may be.
const TITLE_CHARS: usize = 60;

/// What a session is doing, as far as the last recorded event tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
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

    /// The status an event leads to, or `None` if it changes nothing. `waiting` says whether a
    /// permission request is still open once the event has happened.
    fn after(event: &AgentEvent, waiting: bool) -> Option<Self> {
        match event {
            AgentEvent::UserMessage { .. } => Some(Self::Working),
            AgentEvent::Text { .. }
            | AgentEvent::ToolUse { .. }
            | AgentEvent::ToolResult { .. }
            | AgentEvent::PermissionRequest { .. }
            | AgentEvent::PermissionAnswered { .. }
            | AgentEvent::PermissionWithdrawn { .. } => Some(Self::busy(waiting)),
            AgentEvent::TurnDone { .. }
            | AgentEvent::Exited {
                code: None | Some(0),
                ..
            } => Some(Self::Idle),
            AgentEvent::Exited { .. } => Some(Self::Errored),
            AgentEvent::Init { .. }
            | AgentEvent::TextDelta { .. }
            | AgentEvent::RateLimit { .. }
            | AgentEvent::Other { .. } => None,
        }
    }

    fn busy(waiting: bool) -> Self {
        if waiting {
            Self::Waiting
        } else {
            Self::Working
        }
    }
}

/// Applies an event to a session's open permission requests.
fn track(open: &mut HashSet<String>, event: &AgentEvent) {
    match event {
        AgentEvent::PermissionRequest { request_id, .. } => {
            open.insert(request_id.clone());
        }
        AgentEvent::PermissionAnswered { request_id, .. }
        | AgentEvent::PermissionWithdrawn { request_id } => {
            open.remove(request_id);
        }
        AgentEvent::UserMessage { .. }
        | AgentEvent::TurnDone { .. }
        | AgentEvent::Exited { .. } => {
            open.clear();
        }
        _ => {}
    }
}

/// One conversation with an agent, in a task's worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Session {
    /// The database id.
    pub id: i64,
    /// The task whose worktree the session runs in.
    pub task_id: i64,
    /// The id of the agent that runs the session.
    pub agent: String,
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

/// What can be changed about a session after it was created.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionSettings {
    /// The model to run on; `None` is the agent's default.
    pub model: Option<String>,
    /// How much the agent may do without asking.
    pub supervision: Supervision,
}

/// Options for [`Core::session_new`].
#[derive(Debug, Clone, Default)]
pub struct SessionOptions {
    /// The id of the agent to run on; `None` is Claude Code.
    pub agent: Option<String>,
    /// A label; without one, the first message is used.
    pub title: Option<String>,
    /// The model to run on; `None` leaves the agent's default.
    pub model: Option<String>,
    /// How much the agent may do without asking.
    pub supervision: Supervision,
}

const SESSION_SELECT: &str =
    "SELECT id, task_id, title, agent_session_id, model, supervision, status, agent FROM sessions";

fn session_from_row(row: &Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        task_id: row.get(1)?,
        title: row.get(2)?,
        agent_session_id: row.get(3)?,
        model: row.get(4)?,
        supervision: Supervision::parse(&row.get::<_, String>(5)?).unwrap_or_default(),
        status: SessionStatus::parse(&row.get::<_, String>(6)?),
        agent: row.get(7)?,
    })
}

impl Store {
    fn session_insert(&self, task_id: i64, options: &SessionOptions) -> Result<Session> {
        let title = options.title.clone().unwrap_or_default();
        let agent = options.agent.clone().unwrap_or_else(|| CLAUDE.to_string());
        self.conn.execute(
            "INSERT INTO sessions (task_id, title, model, supervision, agent) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                task_id,
                title,
                options.model,
                options.supervision.as_str(),
                agent
            ],
        )?;
        Ok(Session {
            id: self.conn.last_insert_rowid(),
            task_id,
            agent,
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
        let mut stmt = self.conn.prepare(&format!(
            "{SESSION_SELECT} WHERE task_id = ?1 AND closed_at IS NULL ORDER BY id"
        ))?;
        let rows = stmt.query_map(params![task_id], session_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn sessions_all(&self) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(&format!(
            "{SESSION_SELECT} WHERE closed_at IS NULL ORDER BY id"
        ))?;
        let rows = stmt.query_map([], session_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn session_configure(&self, id: i64, settings: &SessionSettings) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET model = ?2, supervision = ?3 WHERE id = ?1",
            params![id, settings.model, settings.supervision.as_str()],
        )?;
        Ok(())
    }

    fn session_close(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET closed_at = CURRENT_TIMESTAMP, status = 'idle' WHERE id = ?1",
            params![id],
        )?;
        Ok(())
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
    fn record(
        &self,
        session_id: i64,
        event: &AgentEvent,
        status: Option<SessionStatus>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO session_events (session_id, event) VALUES (?1, ?2)",
            params![session_id, serde_json::to_string(event)?],
        )?;
        if let Some(status) = status {
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
    /// Creates a session in a task. Nothing runs until [`Core::session_start`].
    pub fn session_new(
        &self,
        project: &str,
        task: &str,
        options: &SessionOptions,
    ) -> Result<Session> {
        let (_, task) = self.task(project, task)?;
        if let Some(agent) = &options.agent {
            self.agents.backend(agent)?;
        }
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

    /// Every event of every running session, from now on.
    ///
    /// A receiver that falls more than [`EVENT_BUFFER`] events behind gets a `Lagged` error
    /// and should reload the affected history with [`Core::session_events`].
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<SessionEvent> {
        self.live.bus.subscribe()
    }

    /// The ids of the sessions whose agent is running.
    #[must_use]
    pub fn running_sessions(&self) -> Vec<i64> {
        let mut ids: Vec<i64> = self.live.commands().keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Starts the agent for a session, continuing its conversation if it has one. Does
    /// nothing if it is already running.
    ///
    /// Every event is stored before it is published, so history survives a front end that
    /// stops listening. Must be called from within a tokio runtime.
    pub fn session_start(&self, id: i64, auth: Auth) -> Result<()> {
        let session = self.session(id)?;
        let task = self
            .store()
            .task_by_id(session.task_id)?
            .ok_or(Error::SessionNotFound(id))?;
        // Held from the check to the insert, so two callers cannot start the same session twice.
        let mut running = self.live.commands();
        if running.contains_key(&id) {
            return Ok(());
        }
        let mcp_servers = self.mcp_servers_for(&session.agent, &task)?;
        let handle = self.agents.backend(&session.agent)?.start(SessionSpec {
            mcp_servers,
            cwd: task.worktree_path,
            model: session.model,
            supervision: session.supervision,
            auth,
            resume: session.agent_session_id,
            ..SessionSpec::default()
        })?;
        let run = self.live.runs.fetch_add(1, Ordering::Relaxed);
        running.insert(id, (run, handle.commands));
        let (store, live) = (Arc::clone(&self.store), Arc::clone(&self.live));
        drop(tokio::spawn(record_and_publish(
            handle.events,
            store,
            live,
            (id, run),
        )));
        Ok(())
    }

    /// The MCP servers Swarmed hands the agent: the ones configured for Claude Code, for every
    /// agent that does not read that configuration itself.
    fn mcp_servers_for(&self, agent: &str, task: &Task) -> Result<Vec<McpServer>> {
        if !self.agents.gets_claude_code_mcp_servers(agent)? {
            return Ok(vec![]);
        }
        let repo = self
            .store()
            .project_by_id(task.project_id)?
            .map(|project| project.repo_path);
        let repo = repo.unwrap_or_else(|| task.worktree_path.clone());
        Ok(mcp::claude_code_servers(
            &self.paths.claude_config,
            &repo,
            &task.worktree_path,
        ))
    }

    /// Starts a turn with `text` in a running session.
    pub fn session_send(&self, id: i64, text: &str) -> Result<()> {
        let commands = self.live.sender(id)?;
        let event = AgentEvent::UserMessage {
            text: text.to_string(),
        };
        self.live.note(&self.store, id, event)?;
        commands
            .send(AgentCommand::UserMessage(text.to_string()))
            .map_err(|_| Error::SessionNotRunning(id))
    }

    /// Lets a pending permission request go ahead. Does nothing if it is no longer pending.
    pub fn session_allow(&self, id: i64, request_id: &str) -> Result<()> {
        let request_id = request_id.to_string();
        self.decide(id, true, AgentCommand::Allow { request_id })
    }

    /// Refuses a pending permission request; the agent is shown `reason`. Does nothing if it
    /// is no longer pending.
    pub fn session_deny(&self, id: i64, request_id: &str, reason: &str) -> Result<()> {
        let (request_id, reason) = (request_id.to_string(), reason.to_string());
        self.decide(id, false, AgentCommand::Deny { request_id, reason })
    }

    /// Records the decision, so every front end sees it at once, then passes it to the agent.
    fn decide(&self, id: i64, allowed: bool, command: AgentCommand) -> Result<()> {
        let commands = self.live.sender(id)?;
        let (AgentCommand::Allow { request_id } | AgentCommand::Deny { request_id, .. }) = &command
        else {
            return Ok(());
        };
        if !self.live.is_open(id, request_id) {
            return Ok(());
        }
        let request_id = request_id.clone();
        let answered = AgentEvent::PermissionAnswered {
            request_id,
            allowed,
        };
        self.live.note(&self.store, id, answered)?;
        commands
            .send(command)
            .map_err(|_| Error::SessionNotRunning(id))
    }

    /// Stops the current turn. The session stays running.
    pub fn session_interrupt(&self, id: i64) -> Result<()> {
        self.live.command(id, AgentCommand::Interrupt)
    }

    /// Stops a session's agent. Its history stays, and [`Core::session_start`] continues it.
    /// Does nothing if it is not running.
    pub fn session_stop(&self, id: i64) {
        // Dropping the last command sender closes the agent's input, which ends it.
        self.live.commands().remove(&id);
    }

    /// Changes a session's model and supervision. A running agent is told at once: the model
    /// applies from its next turn, the supervision from its next action.
    pub fn session_configure(&self, id: i64, settings: &SessionSettings) -> Result<Session> {
        let before = self.session(id)?;
        self.store().session_configure(id, settings)?;
        // Not running is fine: the session starts with the new settings.
        if let Ok(commands) = self.live.sender(id) {
            tell_agent(&commands, &before, settings);
        }
        self.session(id)
    }

    /// Stops a session's agent and takes the session out of every list. Its history is kept.
    pub fn session_close(&self, id: i64) -> Result<()> {
        self.session(id)?;
        self.session_stop(id);
        self.store().session_close(id)
    }
}

/// Passes on the settings that differ from what the session had.
fn tell_agent(
    commands: &mpsc::UnboundedSender<AgentCommand>,
    before: &Session,
    settings: &SessionSettings,
) {
    if before.model != settings.model {
        commands
            .send(AgentCommand::SetModel(settings.model.clone()))
            .ok();
    }
    if before.supervision != settings.supervision {
        commands
            .send(AgentCommand::SetSupervision(settings.supervision))
            .ok();
    }
}

/// How many events a slow subscriber may fall behind before it starts losing them.
pub const EVENT_BUFFER: usize = 4096;

/// One event of one session, as published to every subscriber.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionEvent {
    /// The session the event belongs to.
    pub session_id: i64,
    /// The session's status after this event, if the event changed it.
    pub status: Option<SessionStatus>,
    /// What happened.
    pub event: AgentEvent,
}

/// A running session's command channel, with a number that tells one run from the next.
type Commands = HashMap<i64, (u64, mpsc::UnboundedSender<AgentCommand>)>;

/// The sessions whose agent is running, and the channel their events are published on.
#[derive(Debug)]
pub(crate) struct Live {
    commands: Mutex<Commands>,
    /// The permission requests of each session that nobody has decided yet.
    open_requests: Mutex<HashMap<i64, HashSet<String>>>,
    runs: AtomicU64,
    bus: broadcast::Sender<SessionEvent>,
}

impl Default for Live {
    fn default() -> Self {
        Self {
            commands: Mutex::default(),
            open_requests: Mutex::default(),
            runs: AtomicU64::new(0),
            bus: broadcast::Sender::new(EVENT_BUFFER),
        }
    }
}

impl Live {
    fn commands(&self) -> MutexGuard<'_, Commands> {
        self.commands.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn sender(&self, id: i64) -> Result<mpsc::UnboundedSender<AgentCommand>> {
        self.commands()
            .get(&id)
            .map(|(_, sender)| sender.clone())
            .ok_or(Error::SessionNotRunning(id))
    }

    fn command(&self, id: i64, command: AgentCommand) -> Result<()> {
        self.sender(id)?
            .send(command)
            .map_err(|_| Error::SessionNotRunning(id))
    }

    fn is_open(&self, id: i64, request_id: &str) -> bool {
        let open = lock(&self.open_requests);
        open.get(&id).is_some_and(|ids| ids.contains(request_id))
    }

    /// The session's status after `event`, once its open permission requests reflect it.
    fn status_after(&self, id: i64, event: &AgentEvent) -> Option<SessionStatus> {
        let mut open = lock(&self.open_requests);
        let requests = open.entry(id).or_default();
        track(requests, event);
        let waiting = !requests.is_empty();
        if waiting {
            return SessionStatus::after(event, true);
        }
        open.remove(&id);
        SessionStatus::after(event, false)
    }

    /// Stores an event, unless it is a piece of one that follows whole, then publishes it.
    fn note(&self, store: &Mutex<Store>, session_id: i64, event: AgentEvent) -> Result<()> {
        let status = self.status_after(session_id, &event);
        // Pieces of a reply are only shown; the whole reply is what gets stored.
        let stored = if matches!(event, AgentEvent::TextDelta { .. }) {
            Ok(())
        } else {
            lock(store).record(session_id, &event, status)
        };
        // An error only means nobody is subscribed right now.
        self.bus
            .send(SessionEvent {
                session_id,
                status,
                event,
            })
            .ok();
        stored
    }

    /// Forgets a session whose agent has exited, unless it was restarted in the meantime.
    fn forget(&self, id: i64, run: u64) {
        let mut commands = self.commands();
        if commands
            .get(&id)
            .is_some_and(|(current, _)| *current == run)
        {
            commands.remove(&id);
        }
    }
}

/// Stores and publishes a run's events until its agent exits.
///
/// Takes only the event half of the agent's handle: holding a command sender here would keep
/// the agent's input open, and [`Core::session_stop`] could never end it.
async fn record_and_publish(
    mut events: mpsc::UnboundedReceiver<AgentEvent>,
    store: Arc<Mutex<Store>>,
    live: Arc<Live>,
    (id, run): (i64, u64),
) {
    while let Some(event) = events.recv().await {
        // A full disk must not cut the user off from a running agent: the event is published
        // even if storing it failed.
        live.note(&store, id, event).ok();
    }
    live.forget(id, run);
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
            SessionStatus::after(&exited(Some(0)), false),
            Some(SessionStatus::Idle)
        );
        assert_eq!(
            SessionStatus::after(&exited(None), false),
            Some(SessionStatus::Idle)
        );
        assert_eq!(
            SessionStatus::after(&exited(Some(1)), false),
            Some(SessionStatus::Errored)
        );
    }

    #[test]
    fn a_session_waits_until_every_request_is_decided() {
        let live = Live::default();
        let request = |id: &str| AgentEvent::PermissionRequest {
            request_id: id.into(),
            tool_name: "Bash".into(),
            input: Value::Null,
            description: None,
        };
        let answered = |id: &str| AgentEvent::PermissionAnswered {
            request_id: id.into(),
            allowed: true,
        };
        let result = AgentEvent::ToolResult {
            tool_use_id: "t1".into(),
            content: String::new(),
            is_error: false,
        };
        let statuses: Vec<_> = [
            request("a"),
            request("b"),
            answered("a"),
            result,
            answered("b"),
        ]
        .iter()
        .map(|event| live.status_after(1, event))
        .collect();
        let (waiting, working) = (Some(SessionStatus::Waiting), Some(SessionStatus::Working));
        assert_eq!(statuses, [waiting, waiting, waiting, waiting, working]);
        assert!(!live.is_open(1, "b"));
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

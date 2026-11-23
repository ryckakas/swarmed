//! Agent sessions: creating them, running them, and keeping what they said.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio::task::AbortHandle;

use crate::account_usage::{AccountUsage, UsageHeard};
use crate::agent::{
    AgentBackend, AgentCommand, AgentEvent, Auth, BackgroundState, Effort, Namer, PLAN_TOOL,
    SessionSpec, Supervision,
};
use crate::agents::CLAUDE;
use crate::commands::SlashCommand;
use crate::error::{Error, Result};
use crate::fork;
use crate::git::Head;
use crate::marks::{Mark, MarkColour};
use crate::mcp::{McpServer, McpTransport};
use crate::paths::Paths;
use crate::queue::{self, Queues};
use crate::secrets::SecretEnv;
use crate::settings::Settings;
use crate::store::{Store, Task};
use crate::terminal::Terminals;
use crate::toolserver::{
    SERVER_NAME, SESSION_ENV, SHOW_HTML_FOR_CLAUDE, SOCKET_ENV, TOOL_SERVER_ARG, VIEW_SPOOL_ENV,
    take_views,
};
use crate::{Core, git, lock, peers, publish, remote, views};

/// The kinds of things an agent leaves running that are sub-agents, as Claude Code names them.
const SUB_AGENT_KINDS: [&str; 2] = ["local_agent", "remote_agent"];

/// How long a session title derived from the first message may be.
/// What stands above the user's rules where an agent reads them.
const RULES_HEADING: &str = "The user has set these rules. They apply to everything you do and \
write in this session, and come before your own defaults:";

/// Where, in the data directory, each session has a folder for what its agent is handed as a
/// file and nobody else may read.
const PRIVATE_DIR: &str = "run/agents";

/// How long an agent that was stopped has to go by itself before it is killed. Closing its
/// input asks it to; one that carries on regardless would write into a worktree that is
/// being removed, or wait for ever on an answer nobody will give.
const GRACE: Duration = Duration::from_secs(3);

/// How many of the latest messages sent to an agent are kept, to tell them from messages it
/// says back that were typed elsewhere.
const SENT_KEPT: usize = 32;

const TITLE_CHARS: usize = 60;
/// How long a title the user typed may be.
const MAX_TITLE_CHARS: usize = 120;

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
            | AgentEvent::Renamed { .. }
            | AgentEvent::Marked { .. }
            | AgentEvent::ConfigProposed { .. }
            | AgentEvent::ConfigAnswered { .. }
            | AgentEvent::ConfigChanged { .. }
            | AgentEvent::SupervisionChanged { .. }
            | AgentEvent::Applied { .. }
            | AgentEvent::Suggestion { .. }
            | AgentEvent::Closed
            | AgentEvent::Deleted
            | AgentEvent::Reordered { .. }
            | AgentEvent::AccountUsage { .. }
            | AgentEvent::Background { .. }
            | AgentEvent::View { .. }
            | AgentEvent::Queue { .. }
            | AgentEvent::ContextUsage { .. }
            | AgentEvent::TerminalShared { .. }
            | AgentEvent::TextDelta { .. }
            | AgentEvent::RateLimit { .. }
            | AgentEvent::ExtraUsage { .. }
            | AgentEvent::UsageLimits { .. }
            | AgentEvent::Forked { .. }
            | AgentEvent::Screen { .. }
            | AgentEvent::CommandAsked { .. }
            | AgentEvent::CommandAnswered { .. }
            | AgentEvent::TerminalOnly { .. }
            | AgentEvent::WorktreeMoved { .. }
            | AgentEvent::RemoteControl { .. }
            | AgentEvent::Heard { .. }
            | AgentEvent::SentToBackground { .. }
            // A sub-agent in the foreground works inside its starter's turn, which is busy
            // already; one in the background works between turns, when the session is not.
            | AgentEvent::Sub { .. }
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
    /// How hard the model thinks; `None` is the agent's default.
    pub effort: Option<Effort>,
    /// What the session is doing.
    pub status: SessionStatus,
    /// Whether its agent has finished a turn, or stopped with an error, that the user has not
    /// looked at yet. With many sessions, this is what tells the answered from the merely idle.
    pub unread: bool,
    /// The session that started this one, if an agent did and not the user.
    pub spawned_by: Option<i64>,
    /// Whether the agent is asked for its fast mode: quicker answers at a higher price, where
    /// the agent has such a thing. Off leaves the agent as it is set up.
    pub fast: bool,
    /// The mark an agent or the user put on it, if any.
    pub mark: Option<Mark>,
}

/// What can be changed about a session after it was created.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionSettings {
    /// The model to run on; `None` is the agent's default.
    pub model: Option<String>,
    /// How much the agent may do without asking.
    pub supervision: Supervision,
    /// How hard the model thinks; `None` is the agent's default.
    pub effort: Option<Effort>,
    /// Whether the agent is asked for its fast mode, where it has one.
    pub fast: bool,
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
    /// How hard the model thinks; `None` leaves the agent's default.
    pub effort: Option<Effort>,
    /// Whether the agent is asked for its fast mode, where it has one.
    pub fast: bool,
}

const SESSION_SELECT: &str = "SELECT id, task_id, title, agent_session_id, model, supervision, status, agent, effort, unread, spawned_by, fast, mark_colour, mark_label \
     FROM sessions";

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
        effort: row
            .get::<_, Option<String>>(8)?
            .and_then(|effort| Effort::parse(&effort)),
        unread: row.get(9)?,
        spawned_by: row.get(10)?,
        fast: row.get(11)?,
        mark: row
            .get::<_, Option<String>>(12)?
            .and_then(|colour| MarkColour::parse(&colour))
            .map(|colour| -> rusqlite::Result<Mark> {
                Ok(Mark {
                    colour,
                    label: row.get(13)?,
                })
            })
            .transpose()?,
    })
}

impl Store {
    pub(crate) fn session_insert(&self, task_id: i64, options: &SessionOptions) -> Result<Session> {
        let title = options.title.clone().unwrap_or_default();
        let agent = options.agent.clone().unwrap_or_else(|| CLAUDE.to_string());
        self.conn.execute(
            // At the end of its task's sessions.
            "INSERT INTO sessions (task_id, title, model, supervision, agent, effort, fast, \
             position) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, \
             (SELECT COALESCE(MAX(position), 0) + 1 FROM sessions WHERE task_id = ?1))",
            params![
                task_id,
                title,
                options.model,
                options.supervision.as_str(),
                agent,
                options.effort.map(Effort::as_str),
                options.fast
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
            effort: options.effort,
            status: SessionStatus::Idle,
            unread: false,
            spawned_by: None,
            fast: options.fast,
            mark: None,
        })
    }

    pub(crate) fn session_by_id(&self, id: i64) -> Result<Session> {
        let sql = format!("{SESSION_SELECT} WHERE id = ?1");
        let session = self
            .conn
            .query_row(&sql, params![id], session_from_row)
            .optional()?;
        session.ok_or(Error::SessionNotFound(id))
    }

    /// Whether the session was closed and has not been reopened since.
    pub(crate) fn session_is_closed(&self, id: i64) -> Result<bool> {
        let sql = "SELECT closed_at IS NOT NULL FROM sessions WHERE id = ?1";
        let closed: Option<bool> = self
            .conn
            .query_row(sql, params![id], |row| row.get(0))
            .optional()?;
        closed.ok_or(Error::SessionNotFound(id))
    }

    /// The ids of every session of a task, the closed ones too.
    fn session_ids(&self, task_id: i64) -> Result<Vec<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM sessions WHERE task_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map(params![task_id], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn sessions(&self, task_id: i64) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(&format!(
            "{SESSION_SELECT} WHERE task_id = ?1 AND closed_at IS NULL ORDER BY position, id"
        ))?;
        let rows = stmt.query_map(params![task_id], session_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// How many events are stored, over all sessions.
    pub(crate) fn event_count(&self) -> Result<u64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM session_events", [], |row| row.get(0))?;
        Ok(u64::try_from(count).unwrap_or(0))
    }

    pub(crate) fn sessions_all(&self) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(&format!(
            "{SESSION_SELECT} WHERE closed_at IS NULL ORDER BY task_id, position, id"
        ))?;
        let rows = stmt.query_map([], session_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn session_configure(&self, id: i64, settings: &SessionSettings) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET model = ?2, supervision = ?3, effort = ?4, fast = ?5 \
             WHERE id = ?1",
            params![
                id,
                settings.model,
                settings.supervision.as_str(),
                settings.effort.map(Effort::as_str),
                settings.fast
            ],
        )?;
        Ok(())
    }

    /// Puts a session on another agent. Its model and fast mode were that agent's own, so
    /// they go back to the default.
    fn session_set_agent(&self, id: i64, agent: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET agent = ?2, model = NULL, fast = 0 WHERE id = ?1",
            params![id, agent],
        )?;
        Ok(())
    }

    fn session_rename(&self, id: i64, title: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET title = ?2 WHERE id = ?1",
            params![id, title],
        )?;
        Ok(())
    }

    /// Puts every session that counts as busy back to idle, and says how many there were.
    fn sessions_settle(&self) -> Result<usize> {
        let settled = self.conn.execute(
            "UPDATE sessions SET status = 'idle' WHERE status IN ('working', 'waiting')",
            params![],
        )?;
        Ok(settled)
    }

    /// Records the end of the agent of every session whose history has a report of something
    /// left in the background and no end after it: nothing outlives the agent, and one that
    /// went with its Swarmed never said so. Without it, a front end that reads the history
    /// takes a sub-agent left there to be at work for good.
    fn background_settle(&self) -> Result<()> {
        // The event's type is the first thing in its JSON, which keeps these prefix matches.
        let mut statement = self.conn.prepare(
            "SELECT DISTINCT e.session_id FROM session_events e
             WHERE e.event LIKE '{\"type\":\"background\"%'
               AND NOT EXISTS (
                   SELECT 1 FROM session_events later
                   WHERE later.session_id = e.session_id AND later.id > e.id
                     AND later.event LIKE '{\"type\":\"exited\"%')",
        )?;
        let left = statement.query_map(params![], |row| row.get::<_, i64>(0))?;
        let sessions = left.collect::<rusqlite::Result<Vec<_>>>()?;
        let ended = AgentEvent::Exited {
            code: None,
            stderr: String::new(),
        };
        for session in sessions {
            self.keep(session, &ended)?;
        }
        Ok(())
    }

    fn session_close(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET closed_at = CURRENT_TIMESTAMP, status = 'idle' WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Takes a session and everything said in it out of the database. A session it started
    /// stays, as one nobody started; a cloud task handed off from it stays, without it.
    fn session_delete(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET spawned_by = NULL WHERE spawned_by = ?1",
            params![id],
        )?;
        self.conn.execute(
            "UPDATE cloud_tasks SET session_id = NULL WHERE session_id = ?1",
            params![id],
        )?;
        // The events go with it: they are declared to.
        self.conn
            .execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    fn session_reopen(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET closed_at = NULL WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// A task's closed sessions, the one closed last first.
    fn sessions_closed(&self, task_id: i64) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(&format!(
            "{SESSION_SELECT} WHERE task_id = ?1 AND closed_at IS NOT NULL \
             ORDER BY closed_at DESC, id DESC"
        ))?;
        let rows = stmt.query_map(params![task_id], session_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// How many closed sessions each task has, by task id. Tasks with none are left out.
    pub(crate) fn closed_counts(&self) -> Result<HashMap<i64, usize>> {
        let mut stmt = self.conn.prepare(
            "SELECT task_id, COUNT(*) FROM sessions WHERE closed_at IS NOT NULL GROUP BY task_id",
        )?;
        let count = |row: &Row<'_>| {
            let closed: i64 = row.get(1)?;
            Ok((row.get(0)?, usize::try_from(closed).unwrap_or(0)))
        };
        let rows = stmt.query_map([], count)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The newest record of an agent starting, in this session if there is one, else in any
    /// session on the same agent.
    pub(crate) fn latest_init(
        &self,
        session_id: Option<i64>,
        agent: &str,
    ) -> Result<Option<AgentEvent>> {
        // The event's type is the first thing in its JSON, which keeps this a prefix match.
        let sql = "SELECT e.event FROM session_events e JOIN sessions s ON s.id = e.session_id \
                   WHERE s.agent = ?2 AND e.event LIKE '{\"type\":\"init\"%' \
                   ORDER BY e.session_id = ?1 DESC, e.id DESC LIMIT 1";
        let text: Option<String> = self
            .conn
            .query_row(sql, params![session_id, agent], |row| row.get(0))
            .optional()?;
        Ok(text.map(|text| event_from_json(&text)))
    }

    /// The session's newest event of one type, by the name it is stored under.
    pub(crate) fn latest_event(&self, session_id: i64, kind: &str) -> Result<Option<AgentEvent>> {
        // The event's type is the first thing in its JSON, which keeps this a prefix match.
        let prefix = format!("{{\"type\":\"{kind}\"%");
        let sql = "SELECT event FROM session_events WHERE session_id = ?1 AND event LIKE ?2 \
                   ORDER BY id DESC LIMIT 1";
        let text: Option<String> = self
            .conn
            .query_row(sql, params![session_id, prefix], |row| row.get(0))
            .optional()?;
        Ok(text.map(|text| event_from_json(&text)))
    }

    fn events(&self, session_id: i64) -> Result<Vec<AgentEvent>> {
        let mut stmt = self
            .conn
            .prepare("SELECT event FROM session_events WHERE session_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map(params![session_id], |row| row.get::<_, String>(0))?;
        let texts = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(texts.iter().map(|text| event_from_json(text)).collect())
    }

    /// Stores an event and nothing else: the session's row stays as it is.
    fn keep(&self, session_id: i64, event: &AgentEvent) -> Result<()> {
        self.conn.execute(
            "INSERT INTO session_events (session_id, event) VALUES (?1, ?2)",
            params![session_id, serde_json::to_string(event)?],
        )?;
        Ok(())
    }

    /// Stores an event and applies what it means for the session's row.
    fn record(
        &self,
        session_id: i64,
        event: &AgentEvent,
        status: Option<SessionStatus>,
        unread: Option<bool>,
    ) -> Result<()> {
        self.keep(session_id, event)?;
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
            // Whoever writes to a session has seen where it stood, and starts something new:
            // a mark was about what came before.
            AgentEvent::UserMessage { text, .. } => self.conn.execute(
                "UPDATE sessions SET unread = 0, mark_colour = NULL, mark_label = '', \
                 title = CASE WHEN title = '' THEN ?2 ELSE title END \
                 WHERE id = ?1",
                params![session_id, title_from(text)],
            )?,
            _ if unread == Some(true) => self.conn.execute(
                "UPDATE sessions SET unread = 1 WHERE id = ?1",
                params![session_id],
            )?,
            _ => 0,
        };
        Ok(())
    }

    /// Starts session `to` with a copy of `events`, said in session `from`, and marks where
    /// the copy ends. A copy is nothing new to read.
    pub(crate) fn session_copy_events(
        &self,
        to: i64,
        events: &[AgentEvent],
        from: i64,
    ) -> Result<()> {
        let title: String = self.conn.query_row(
            "SELECT title FROM sessions WHERE id = ?1",
            params![to],
            |row| row.get(0),
        )?;
        for event in events {
            self.record(to, event, None, None)?;
        }
        self.record(to, &AgentEvent::Forked { from }, None, None)?;
        self.conn.execute(
            "UPDATE sessions SET unread = 0, title = ?2 WHERE id = ?1",
            params![to, title],
        )?;
        Ok(())
    }

    /// Records that session `id` was started by session `parent`.
    pub(crate) fn session_set_spawned_by(&self, id: i64, parent: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET spawned_by = ?2 WHERE id = ?1",
            params![id, parent],
        )?;
        Ok(())
    }

    /// How many open sessions were started by session `parent`.
    pub(crate) fn sessions_spawned_by(&self, parent: i64) -> Result<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE spawned_by = ?1 AND closed_at IS NULL",
            params![parent],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(count).unwrap_or(0))
    }

    fn session_mark_read(&self, id: i64) -> Result<()> {
        self.conn
            .execute("UPDATE sessions SET unread = 0 WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Puts `mark` on a session, or takes its mark off.
    pub(crate) fn session_set_mark(&self, id: i64, mark: Option<&Mark>) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET mark_colour = ?2, mark_label = ?3 WHERE id = ?1",
            params![
                id,
                mark.map(|mark| mark.colour.as_str()),
                mark.map_or("", |mark| mark.label.as_str())
            ],
        )?;
        Ok(())
    }
}

/// Whether `event` is the agent handing the conversation back: a turn that ended, or an agent
/// that stopped with an error. Until the user looks, the session is unread.
fn leaves_something_to_read(event: &AgentEvent) -> bool {
    match event {
        AgentEvent::TurnDone { .. } => true,
        AgentEvent::Exited { code, .. } => !matches!(code, None | Some(0)),
        _ => false,
    }
}

/// An event written by a newer version may not parse; it is shown as [`AgentEvent::Other`].
pub(crate) fn event_from_json(text: &str) -> AgentEvent {
    serde_json::from_str(text).unwrap_or_else(|_| AgentEvent::Other {
        raw: serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string())),
    })
}

/// A session's first title: the start of what the user typed. What they pasted large, which a
/// front end sends wrapped in `<pasted_text_N>` tags, is not what the session is called after.
fn title_from(message: &str) -> String {
    let title = shortened(&without_pastes(message), TITLE_CHARS);
    if title.is_empty() && message.contains("<pasted_text_") {
        return "Pasted text".to_string();
    }
    title
}

/// `message` without the large pastes in it.
fn without_pastes(message: &str) -> String {
    let mut typed = String::new();
    let mut rest = message;
    while let Some((before, paste)) = rest.split_once("<pasted_text_") {
        typed.push_str(before);
        let after = paste.split_once('>').and_then(|(number, after)| {
            let (_, after) = after.split_once(&format!("</pasted_text_{number}>"))?;
            Some(after)
        });
        // An opening tag that is never closed is only text.
        let Some(after) = after else {
            typed.push_str("<pasted_text_");
            rest = paste;
            continue;
        };
        rest = after;
    }
    typed.push_str(rest);
    typed
}

pub(crate) fn is_busy_status(status: SessionStatus) -> bool {
    matches!(status, SessionStatus::Working | SessionStatus::Waiting)
}

/// A namer's answer to naming `message` as a title, or `None` if it is none: empty, markup
/// such as the `<invoke_skill_or_bash_placeholder>` a model without tools answers in place of
/// a tool call, a command, or a command's message repeated.
pub(crate) fn title_of(answer: &str, message: &str) -> Option<String> {
    let title = shortened(answer, TITLE_CHARS);
    let command = message.trim_start().strip_prefix('/');
    let repeated =
        command.is_some_and(|command| title.eq_ignore_ascii_case(&shortened(command, TITLE_CHARS)));
    let refused = title.is_empty() || title.starts_with(['<', '/']) || repeated;
    (!refused).then_some(title)
}

/// The first line of `text`, without surrounding space or quotes, at most `limit` characters.
fn shortened(text: &str, limit: usize) -> String {
    let line = text.trim().lines().next().unwrap_or_default();
    let line = line.trim().trim_matches(['"', '\'', '`']).trim();
    if line.chars().count() <= limit {
        return line.to_string();
    }
    let mut title: String = line.chars().take(limit - 1).collect();
    title.push('…');
    title
}

/// Stores a session's new title and tells every front end.
fn rename(store: &Mutex<Store>, live: &Live, id: i64, title: String) -> Result<()> {
    lock(store).session_rename(id, &title)?;
    live.note(store, id, AgentEvent::Renamed { title })
}

/// Asks `namer` to name `asked`, what stands for the session's first message, and uses the
/// answer, unless it is no title or the session was renamed in the meantime.
async fn name_session(
    namer: Arc<dyn Namer>,
    store: Arc<Mutex<Store>>,
    live: Arc<Live>,
    (id, prompt, asked): (i64, String, String),
) {
    let from_prompt = title_from(&prompt);
    let suggested = namer.name(asked).await;
    let Some(title) = suggested.and_then(|answer| title_of(&answer, &prompt)) else {
        return;
    };
    let current = lock(&store).session_by_id(id).map(|session| session.title);
    if current.is_ok_and(|current| current == from_prompt) {
        rename(&store, &live, id, title).ok();
    }
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
            self.agents_now().backend(agent)?;
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

    /// The commands and skills a message to this session can call by starting with `/`.
    ///
    /// They are known from the last time the session's agent started. A session that has not
    /// run yet, or `None`, gets what the newest session on the same agent reported, which may
    /// differ where projects bring their own commands.
    pub fn slash_commands(&self, session: Option<i64>) -> Result<Vec<SlashCommand>> {
        let agent = match session {
            Some(id) => self.session(id)?.agent,
            None => CLAUDE.to_string(),
        };
        let init = self.store().latest_init(session, &agent)?;
        let mut commands = match init {
            Some(AgentEvent::Init { slash_commands, .. }) => slash_commands,
            _ => vec![],
        };
        // The skills Swarmed hands over when called by name: the ones shared with this agent,
        // and for an agent that reports no commands of its own, its own skills as well.
        let worktree = match session {
            Some(id) => self.worktree_of_session(id),
            None => None,
        };
        let skills = self.paths.skills_by_name(&agent, worktree.as_deref());
        let known: HashSet<String> = commands.iter().cloned().collect();
        let names = skills.into_iter().map(|skill| skill.name);
        commands.extend(names.filter(|name| !known.contains(name)));
        Ok(crate::commands::offered(&agent, commands))
    }

    /// Where session `id` works, if its task is still there.
    fn worktree_of_session(&self, id: i64) -> Option<PathBuf> {
        let session = self.session(id).ok()?;
        let task = self.store().task_by_id(session.task_id).ok()??;
        Some(task.worktree_path)
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
    /// nothing if it is already running. A closed session is refused: it has to be reopened
    /// first, so that no agent is at work where no list shows it.
    ///
    /// Every event is stored before it is published, so history survives a front end that
    /// stops listening. Must be called from within a tokio runtime.
    pub fn session_start(&self, id: i64, auth: Auth) -> Result<()> {
        let session = self.session(id)?;
        if self.store().session_is_closed(id)? {
            return Err(Error::SessionClosed(id));
        }
        // Most calls find the agent running, and then nothing needs working out.
        if self.live.is_running(id) {
            return Ok(());
        }
        let task = self
            .store()
            .task_by_id(session.task_id)?
            .ok_or(Error::SessionNotFound(id))?;
        let spool = self.paths.view_spool(id);
        let backend = self.agents_now().backend(&session.agent)?;
        // Worked out before the running sessions are locked: reading settings, other agents'
        // configuration and the keychain takes a while, and nobody else's message, answer or
        // stop is to wait for that.
        let spec = self.spec_for(session, task, &spool, auth)?;
        let shared = Shared {
            store: Arc::clone(&self.store),
            live: Arc::clone(&self.live),
            terminals: Arc::clone(&self.terminals),
            paths: self.paths.clone(),
            spool,
            backend,
            spec,
        };
        launch(shared, id, Launch::UnlessRunning)
    }

    /// Everything the agent of `session`, which works in `task`, is started with.
    fn spec_for(
        &self,
        session: Session,
        task: Task,
        spool: &Path,
        auth: Auth,
    ) -> Result<SessionSpec> {
        let settings = self.settings()?;
        let mut mcp_servers = self.mcp_servers_for(&session.agent, &task)?;
        let own = self.own_mcp_servers()?;
        // One set up here goes before another agent's of the same name.
        mcp_servers.retain(|shared| own.iter().all(|server| server.name != shared.name));
        mcp_servers.extend(own);
        mcp_servers.extend(self.own_tools(session.id, spool));
        let mut env = if session.agent == CLAUDE {
            SecretEnv::default()
        } else {
            self.key_env()?
        };
        // What the task's environment sets for its hooks, the agent working in it gets too.
        env.0.extend(self.task_env(&task)?);
        Ok(SessionSpec {
            mcp_servers,
            skills: self.skills_had(&session.agent, &task.worktree_path),
            env,
            append_system_prompt: Some(self.guidance(&settings, &session.agent, &task)),
            cwd: task.worktree_path,
            model: session.model,
            supervision: session.supervision,
            effort: session.effort,
            fast: session.fast,
            auth,
            resume: session.agent_session_id,
            // Showing a view changes nothing, so it is never worth a question.
            allowed_tools: vec![SHOW_HTML_FOR_CLAUDE.to_string()],
            private_dir: Some(self.private_dir(session.id)),
            ..SessionSpec::default()
        })
    }

    /// The folder for what the agent of session `id` is handed as a file.
    fn private_dir(&self, id: i64) -> PathBuf {
        let folder = self.paths.data_dir.join(PRIVATE_DIR);
        folder.join(id.to_string())
    }

    /// What every agent is told by Swarmed, on top of what it knows by itself: the user's own
    /// rules, if they wrote any; to keep to its task's branch and worktree; about the other sessions, when
    /// there is a tool server to reach them through; and about showing views, when the user has
    /// those switched on.
    fn guidance(&self, settings: &Settings, agent: &str, task: &Task) -> String {
        let rules = settings.rules.trim();
        let rules = (!rules.is_empty()).then(|| format!("{RULES_HEADING}\n{rules}"));
        let branch = publish::worktree_instructions(&task.branch);
        let sessions = self.tool_server.as_ref().map(|_| peers::INSTRUCTIONS);
        // Handed over through the tool server, so only worth naming when there is one.
        let skills = sessions.and_then(|_| self.skills_guidance(agent, &task.worktree_path));
        let views = settings.html_views.then_some(views::INSTRUCTIONS);
        let parts = [
            rules.as_deref(),
            Some(branch.as_str()),
            sessions,
            skills.as_deref(),
            views,
        ];
        let parts: Vec<&str> = parts.into_iter().flatten().collect();
        parts.join("\n\n")
    }

    /// Swarmed's own tool server, for every agent, when a front end said where it is. It is
    /// told which session it serves, where to leave views, and where the core listens.
    fn own_tools(&self, session: i64, spool: &Path) -> Option<McpServer> {
        let executable = self.tool_server.as_ref()?;
        let socket = self.paths.tools_socket();
        let env = [
            (VIEW_SPOOL_ENV, spool.to_string_lossy().into_owned()),
            (SESSION_ENV, session.to_string()),
            (SOCKET_ENV, socket.to_string_lossy().into_owned()),
        ];
        Some(McpServer {
            name: SERVER_NAME.to_string(),
            transport: McpTransport::Stdio {
                command: executable.to_string_lossy().into_owned(),
                args: vec![TOOL_SERVER_ARG.to_string()],
                env: env
                    .into_iter()
                    .map(|(name, value)| (name.to_string(), value))
                    .collect(),
            },
        })
    }

    /// The MCP servers of the other agents that a session on `agent` lacks: see `mcp_pool`.
    /// Each agent's are read from its own configuration now, so one added there since is in.
    fn mcp_servers_for(&self, agent: &str, task: &Task) -> Result<Vec<McpServer>> {
        if !self.agents_now().gets_shared_mcp_servers(agent)? {
            return Ok(vec![]);
        }
        let repo = self
            .store()
            .project_by_id(task.project_id)?
            .map(|project| project.repo_path);
        let repo = repo.unwrap_or_else(|| task.worktree_path.clone());
        let paths = &self.paths;
        Ok(paths.shared_mcp_servers(agent, &repo, &task.worktree_path))
    }

    /// Gives a session a title of the user's choosing.
    pub fn session_rename(&self, id: i64, title: &str) -> Result<Session> {
        self.session(id)?;
        let title = shortened(title, MAX_TITLE_CHARS);
        if title.is_empty() {
            return Err(Error::EmptyTitle);
        }
        rename(&self.store, &self.live, id, title)?;
        self.session(id)
    }

    /// Has the session named after its first message, if the user wants that and something
    /// can do it. Until the name arrives the title is the start of the message.
    fn name_later(&self, id: i64, prompt: &str) {
        let wanted = self.settings().is_ok_and(|settings| settings.auto_title);
        let Some(namer) = self.namer.clone().filter(|_| wanted) else {
            return;
        };
        let asked = self.naming_prompt_for(id, prompt);
        let (store, live) = (Arc::clone(&self.store), Arc::clone(&self.live));
        let naming = name_session(namer, store, live, (id, prompt.to_string(), asked));
        drop(tokio::spawn(naming));
    }

    /// What the namer is asked to name for `prompt`, the first message of session `id`, with
    /// the session's agent and worktree to find a skill it calls by.
    fn naming_prompt_for(&self, id: i64, prompt: &str) -> String {
        let session = self.store().session_by_id(id).ok();
        let task = session
            .as_ref()
            .and_then(|session| self.store().task_by_id(session.task_id).ok().flatten());
        let agent = session.as_ref().map(|session| session.agent.as_str());
        let worktree = task.as_ref().map(|task| task.worktree_path.as_path());
        self.naming_prompt(prompt, agent, worktree)
    }

    /// Starts a turn with `text` in a running session. A message that is one of the agent's
    /// own commands which Swarmed shows or asks about itself starts no turn: that is done
    /// instead.
    pub fn session_send(&self, id: i64, text: &str) -> Result<()> {
        if self.command_taken(id, text, None)? {
            return Ok(());
        }
        self.session_send_as_typed(id, text)
    }

    /// Starts a turn with `text` in a running session, whatever `text` is.
    pub(crate) fn session_send_as_typed(&self, id: i64, text: &str) -> Result<()> {
        self.live.sender(id)?;
        if self.session(id)?.title.is_empty() {
            self.name_later(id, text);
        }
        let with = (&*self.terminals, &self.paths);
        send_text(&self.store, &self.live, with, id, text)
    }

    /// Lets a pending permission request go ahead. Does nothing if it is no longer pending.
    pub fn session_allow(&self, id: i64, request_id: &str) -> Result<()> {
        let request_id = request_id.to_string();
        self.decide(id, true, AgentCommand::Allow { request_id })
    }

    /// Answers the questions a pending request puts to the user, each by its text: the
    /// agent's tool goes ahead with them. Does nothing if the request is no longer pending.
    pub fn session_answer(
        &self,
        id: i64,
        request_id: &str,
        answers: BTreeMap<String, String>,
    ) -> Result<()> {
        let request_id = request_id.to_string();
        let command = AgentCommand::Answer {
            request_id,
            answers,
        };
        self.decide(id, true, command)
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
        let (AgentCommand::Allow { request_id }
        | AgentCommand::Answer { request_id, .. }
        | AgentCommand::Deny { request_id, .. }) = &command
        else {
            return Ok(());
        };
        if !self.live.is_open(id, request_id) {
            return Ok(());
        }
        let request_id = request_id.clone();
        let answered = AgentEvent::PermissionAnswered {
            request_id: request_id.clone(),
            allowed,
        };
        let plan_approved = allowed && self.is_plan_request(id, &request_id)?;
        self.live.note(&self.store, id, answered)?;
        commands
            .send(command)
            .map_err(|_| Error::SessionNotRunning(id))?;
        if plan_approved {
            self.leave_plan_mode(id)?;
        }
        Ok(())
    }

    /// Whether `request_id` is the session's agent presenting its plan.
    fn is_plan_request(&self, id: i64, request_id: &str) -> Result<bool> {
        // A plan is the only thing asked while it is open, so it is the latest request.
        let latest = self.store().latest_event(id, "permission_request")?;
        Ok(matches!(
            latest,
            Some(AgentEvent::PermissionRequest { request_id: asked, tool_name, .. })
                if asked == request_id && tool_name == PLAN_TOOL
        ))
    }

    /// Once its plan is approved the agent carries it out, supervised the way new sessions are.
    /// The agent is told, since by itself it would fall back to asking about everything.
    fn leave_plan_mode(&self, id: i64) -> Result<()> {
        let session = self.session(id)?;
        if session.supervision != Supervision::Plan {
            return Ok(());
        }
        let supervision = match self.settings()?.default_supervision {
            Supervision::Plan => Supervision::Ask,
            other => other,
        };
        let settings = SessionSettings {
            model: session.model,
            supervision,
            effort: session.effort,
            fast: session.fast,
        };
        self.session_configure(id, &settings).map(drop)
    }

    /// Puts a session that has not begun on another agent, with that agent's default model.
    ///
    /// Only before the first message reaches an agent: from then on the agent holds the
    /// conversation, and another would not know what was said. A fork has not begun either,
    /// since what it starts from is handed to its agent as text.
    pub fn session_set_agent(&self, id: i64, agent: &str) -> Result<Session> {
        let session = self.session(id)?;
        if session.agent == agent {
            return Ok(session);
        }
        self.agents_now().backend(agent)?;
        let running = self.live.sender(id).is_ok();
        if running || session.agent_session_id.is_some() {
            return Err(Error::SessionBegun(id));
        }
        self.store().session_set_agent(id, agent)?;
        self.session(id)
    }

    /// Stops the current turn. The session stays running, and the first message waiting in
    /// its queue goes the moment the turn has ended, as after a turn that ended well.
    ///
    /// A session that counts as busy though no agent runs for it, which a Swarmed that ended
    /// in the middle of a turn leaves behind, is put right instead: its turn is recorded as
    /// stopped, and its queue left as it is.
    pub fn session_interrupt(&self, id: i64) -> Result<()> {
        let busy = || {
            self.session(id)
                .is_ok_and(|session| is_busy_status(session.status))
        };
        // Whether the turn is under way is asked before it is stopped, and under the lock its
        // end is dealt with under, so the end finds the session marked however soon it comes.
        let mut pressing = self.live.queues.pressing();
        let under_way = busy();
        let stopped = self.live.command(id, AgentCommand::Interrupt);
        if stopped.is_ok() && under_way {
            pressing.insert(id);
        }
        drop(pressing);
        if stopped.is_ok() || !busy() {
            return stopped;
        }
        let ended = AgentEvent::TurnDone {
            is_error: true,
            result: Some("the agent was no longer running".to_string()),
            cost_usd: None,
            duration_ms: None,
        };
        self.live.note(&self.store, id, ended)
    }

    /// Puts the sessions that count as busy back to idle: for a front end that has just
    /// started, when no agent runs yet and whatever says otherwise is left from a Swarmed that
    /// ended in the middle of a turn. Says how many there were. What their agents left in the
    /// background went with them, which is recorded as the end those agents never reported.
    pub fn sessions_settle(&self) -> Result<usize> {
        let store = self.store();
        store.background_settle()?;
        store.sessions_settle()
    }

    /// Stops a session's agent. Its history stays, and [`Core::session_start`] continues it.
    /// Does nothing if it is not running.
    ///
    /// The agent is asked to end by closing its input, and counts as stopped at once. One
    /// that has not gone a few seconds later is killed.
    pub fn session_stop(&self, id: i64) {
        // A turn stopped to send a waiting message ends with the agent: the message waits.
        self.live.queues.pressing().remove(&id);
        self.live.stop(id);
    }

    /// Deletes what a session's agent was handed as files, which can hold the secrets of its
    /// MCP servers, and the views it showed that nobody read. Best effort: the session is
    /// going, or its task is, and nothing reads them again.
    fn forget_session_files(&self, id: i64) {
        std::fs::remove_dir_all(self.paths.view_spool(id)).ok();
        std::fs::remove_dir_all(self.private_dir(id)).ok();
    }

    /// Stops the agents of every session of a task, closed ones still on their way out
    /// included, and waits until they have gone: by themselves, or killed for taking too
    /// long. For when the task's worktree is about to go, so that nothing writes into it.
    pub(crate) async fn sessions_ended(&self, task_id: i64) -> Result<()> {
        let ids = self.store().session_ids(task_id)?;
        let mut endings = vec![];
        for id in ids {
            self.session_stop(id);
            self.terminal_close(id);
            endings.extend(self.live.endings_of(id));
        }
        for ending in endings {
            ending.gone().await;
        }
        Ok(())
    }

    /// Deletes what the sessions of a removed task left behind: messages waiting to be sent,
    /// unread views and private files. Only once the task is gone: a removal that was refused
    /// leaves its sessions as they were.
    pub(crate) fn sessions_forgotten(&self, task_id: i64) -> Result<()> {
        // Read first: the store is let go before the loop, which records in it.
        let ids = self.store().session_ids(task_id)?;
        for id in ids {
            self.live.queues.clear(id);
            self.forget_session_files(id);
            // Its sessions are in no list any more: none is to be started again for it.
            self.remote_control_closing(id)?;
        }
        Ok(())
    }

    /// Changes a session's model, supervision and effort. A running agent is told at once: the
    /// model applies from its next turn, the supervision from its next action. An agent takes
    /// its effort only when it starts, so a running one is stopped once its turn is over and
    /// continues the conversation, with the new effort, on the next message.
    pub fn session_configure(&self, id: i64, settings: &SessionSettings) -> Result<Session> {
        let before = self.session(id)?;
        self.store().session_configure(id, settings)?;
        // Not running is fine: the session starts with the new settings.
        if let Ok(commands) = self.live.sender(id) {
            // A running agent knows the default it was started with, and the list it comes
            // from may have changed since.
            let default_model = self.agents_now().default_model(&before.agent);
            tell_agent(&commands, &before, settings, default_model);
        }
        // Like its effort, an agent takes its fast mode when it starts.
        if before.effort != settings.effort || before.fast != settings.fast {
            self.live.restart_when_idle(id, before.status);
        }
        if before.supervision != settings.supervision {
            let supervision = settings.supervision;
            let changed = AgentEvent::SupervisionChanged { supervision };
            self.live.note(&self.store, id, changed)?;
        }
        self.session(id)
    }

    /// Notes that the user has looked at the session, so it no longer counts as unread.
    pub fn session_mark_read(&self, id: i64) -> Result<()> {
        self.session(id)?;
        self.store().session_mark_read(id)
    }

    /// A task's closed sessions, the one closed last first.
    pub fn sessions_closed(&self, project: &str, task: &str) -> Result<Vec<Session>> {
        let (_, task) = self.task(project, task)?;
        self.store().sessions_closed(task.id)
    }

    /// Puts a closed session back in the lists, as it was: its conversation was kept, and its
    /// agent starts again with the next message.
    pub fn session_reopen(&self, id: i64) -> Result<Session> {
        self.session(id)?;
        self.store().session_reopen(id)?;
        self.session(id)
    }

    /// Stops a session's agent and takes the session out of every list. Its history is kept.
    pub fn session_close(&self, id: i64) -> Result<()> {
        self.session(id)?;
        self.remote_control_closing(id)?;
        self.session_stop(id);
        self.terminal_close(id);
        self.live.queues.clear(id);
        self.store().session_close(id)?;
        self.live.note(&self.store, id, AgentEvent::Closed)
    }

    /// Deletes a session for good, open or closed: its agent is stopped, and its conversation
    /// and what Swarmed kept for it are gone. The task's files and branch are not touched, and
    /// neither is what the agent keeps of the conversation in its own folders.
    pub fn session_delete(&self, id: i64) -> Result<()> {
        self.session(id)?;
        self.session_stop(id);
        // Its conversation is going, so the agent is not given time to finish anything: it
        // is killed, and whatever it still says meanwhile is dropped. That holds for an
        // earlier run that is still on its way out too.
        for ending in self.live.endings_of(id) {
            ending.discard();
            ending.kill();
        }
        // What its agent would still say is dropped, the end of Remote Control with it.
        self.live.remote_ends(id);
        self.terminal_close(id);
        self.live.queues.clear(id);
        // Views the agent showed and nobody has read yet. Gone already is as good as removed.
        match std::fs::remove_dir_all(self.paths.view_spool(id)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        std::fs::remove_dir_all(self.private_dir(id)).ok();
        self.store().session_delete(id)?;
        // Only told, not recorded: there is no session left to record it in.
        self.live.note(&self.store, id, AgentEvent::Deleted)
    }
}

/// Starts a turn with `text` in a running session: records the message, then hands it to the
/// agent together with what happened in the session's terminal since its last turn.
pub(crate) fn send_text(
    store: &Mutex<Store>,
    live: &Live,
    (terminals, paths): (&Terminals, &Paths),
    id: i64,
    text: &str,
) -> Result<()> {
    let commands = live.sender(id)?;
    // A message that calls a skill the agent would not know by name is sent with what the
    // skill says. Worked out before anything is recorded; what is recorded is what was typed.
    let called = skill_called(store, paths, id, text);
    // A fork's agent has not heard the conversation it starts from: it is told with the first
    // message. Looked up before the message is recorded, and the user's own words are
    // recorded as they typed them.
    let earlier = fork::carried(&lock(store).events(id)?);
    let event = AgentEvent::UserMessage {
        text: text.to_string(),
        remote: false,
    };
    live.note(store, id, event)?;
    let shared = terminals.take_unshared(id);
    let moved = moved_since(store, live, id);
    let text = called.as_deref().unwrap_or(text);
    let text = &earlier.map_or_else(
        || text.to_string(),
        |earlier| fork::with_carried(text, &earlier),
    );
    let message = if shared.is_empty() {
        text.clone()
    } else {
        live.note(
            store,
            id,
            AgentEvent::TerminalShared {
                text: shared.clone(),
            },
        )?;
        format!(
            "{text}\n\n<terminal>\nWhat the user ran in this session's terminal since your last \
             turn, with its output:\n{shared}\n</terminal>"
        )
    };
    let message = match moved {
        Some((Head(from), Head(to))) => {
            let note = moved_note(from.as_deref(), to.as_deref());
            let between_turns = true;
            live.note(
                store,
                id,
                AgentEvent::WorktreeMoved {
                    from,
                    to,
                    between_turns,
                },
            )?;
            format!("{message}\n\n{note}")
        }
        None => message,
    };
    live.sent(id, &message);
    commands
        .send(AgentCommand::UserMessage(message))
        .map_err(|_| Error::SessionNotRunning(id))
}

/// The move of the task's worktree to another branch since the agent of session `id` last knew
/// where it was, if there was one: by the user, from Swarmed or by hand, or by another session.
/// The agent knows where it is from now on, and so does the watch for moves.
fn moved_since(store: &Mutex<Store>, live: &Live, id: i64) -> Option<(Head, Head)> {
    let worktree = {
        let store = lock(store);
        let session = store.session_by_id(id).ok()?;
        store.task_by_id(session.task_id).ok()??.worktree_path
    };
    let now = git::head_now(&worktree)?;
    live.branch_seen(&worktree, &now);
    let before = live.branch_known(id, &now)?;
    Some((before, now))
}

/// What an agent is told along with the user's message when its worktree moved since its last
/// turn, so that it does not carry on as if it were where it left off.
fn moved_note(from: Option<&str>, to: Option<&str>) -> String {
    let on =
        |branch: Option<&str>| branch.map_or_else(|| "no branch".to_string(), |b| format!("`{b}`"));
    format!(
        "<worktree>\nThis task's worktree was moved since your last turn, by the user or by \
         something other than you: it was on {}, and is on {} now, with that branch's files. \
         Carry on there unless the user says otherwise.\n</worktree>",
        on(from),
        on(to)
    )
}

/// What the agent of session `id` is sent in place of `text`, if `text` calls a skill by name.
pub(crate) fn skill_called(
    store: &Mutex<Store>,
    paths: &Paths,
    id: i64,
    text: &str,
) -> Option<String> {
    if !text.starts_with('/') {
        return None;
    }
    let store = lock(store);
    let session = store.session_by_id(id).ok()?;
    let task = store.task_by_id(session.task_id).ok()??;
    drop(store);
    paths.skill_called(&session.agent, &task.worktree_path, text)
}

/// Passes on the settings that differ from what the session had. A model left open is passed
/// on as `default_model`, where Swarmed has one for the agent.
fn tell_agent(
    commands: &mpsc::UnboundedSender<AgentCommand>,
    before: &Session,
    settings: &SessionSettings,
    default_model: Option<String>,
) {
    if before.model != settings.model {
        let model = settings.model.clone().or(default_model);
        commands.send(AgentCommand::SetModel(model)).ok();
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
    /// Whether the session is unread after this event, if the event changed that: an answer
    /// handed back makes it unread and a message of the user's makes it read. A turn that ends
    /// while sub-agents work on in the background hands nothing back yet, and changes nothing.
    pub unread: Option<bool>,
    /// What happened.
    pub event: AgentEvent,
}

/// One run of a session's agent, for as long as it counts as running.
#[derive(Debug)]
struct Run {
    /// Tells this run from the session's earlier and later ones.
    number: u64,
    commands: mpsc::UnboundedSender<AgentCommand>,
    /// Never sent on. It goes when the run is dropped from the running ones, which is what
    /// tells the task following the run that it was stopped.
    _stop: oneshot::Sender<()>,
}

/// The running sessions' agents, by session.
type Commands = HashMap<i64, Run>;

/// A hold on the end of one run: for making it certain, and for waiting until it has come.
#[derive(Debug, Clone)]
struct Ending {
    /// The tasks that hold the agent's process.
    tasks: Vec<AbortHandle>,
    /// Closed when the run's last event has been dealt with.
    gone: watch::Receiver<()>,
    /// Set when the session was deleted: what the run still says has nowhere to go.
    discarded: Arc<AtomicBool>,
}

impl Ending {
    /// Kills the agent. Nothing more is heard from it.
    fn kill(&self) {
        for task in &self.tasks {
            task.abort();
        }
    }

    /// Has whatever the run still says dropped instead of stored and published.
    fn discard(&self) {
        self.discarded.store(true, Ordering::Relaxed);
    }

    /// Waits until the run is over. It is, a little after [`GRACE`] at the latest, once it
    /// was stopped; the limit here is for a run that was not.
    async fn gone(mut self) {
        // Nothing is ever sent, so this waits for the sender to go, which it does with the
        // task that follows the run.
        let over = self.gone.wait_for(|()| false);
        tokio::time::timeout(GRACE * 3, over).await.ok();
    }
}

/// How what a run says is to be taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Standing {
    /// It is the session's agent: the one running, or the last that ran.
    Current,
    /// Another run of the session has started since. This one no longer says how the
    /// session stands.
    Replaced,
    /// The session was deleted.
    Discarded,
}

/// Whether `event` is only ever shown, and not part of what a session keeps: the pieces of a
/// reply, whose whole follows; a queue; and what only tells front ends to read something again.
fn shown_only(event: &AgentEvent) -> bool {
    matches!(
        event,
        AgentEvent::TextDelta { .. }
            | AgentEvent::Queue { .. }
            | AgentEvent::ConfigChanged { .. }
            | AgentEvent::Deleted
            | AgentEvent::Reordered { .. }
            | AgentEvent::AccountUsage { .. }
    )
}

/// What an agent answered to sending work to the background: whether it went, and why not if
/// the agent refused.
pub(crate) type Sent = (bool, Option<String>);

/// Someone waiting for an agent to answer whether work went to the background.
#[derive(Debug)]
struct Sending {
    /// The call whose work was to go; `None` for every call.
    call: Option<String>,
    answer: oneshot::Sender<Sent>,
}

/// A message told to an agent in the middle of its turn.
#[derive(Debug)]
pub(crate) struct Told {
    /// What the agent was sent, and says back when it takes the message in.
    pub(crate) sent: String,
    /// What the user typed, which is what is recorded.
    pub(crate) typed: String,
}

/// The sessions whose agent is running, and the channel their events are published on.
#[derive(Debug)]
pub(crate) struct Live {
    commands: Mutex<Commands>,
    /// Every run that has not ended yet, by its number, with its session: the running ones,
    /// and the ones that were stopped and are on their way out.
    endings: Mutex<HashMap<u64, (i64, Ending)>>,
    /// The permission requests of each session that nobody has decided yet.
    open_requests: Mutex<HashMap<i64, HashSet<String>>>,
    /// Sessions whose agent is to be stopped when its turn ends, to start again with new settings.
    restarts: Mutex<HashSet<i64>>,
    /// The messages waiting for each session's turn to end.
    pub(crate) queues: Queues,
    /// The process of each session's agent, as of when it was last started.
    pub(crate) pids: Mutex<HashMap<i64, u32>>,
    /// The stack each task has running, by task: what its `start` command started.
    pub(crate) stacks: Mutex<HashMap<i64, std::process::Child>>,
    /// When the turn each session is in began: when the message that started it was sent.
    turns: Mutex<HashMap<i64, Instant>>,
    /// The branch each task's worktree was last seen on (`None`: no branch), by its folder:
    /// a move is what differs from it.
    branches_seen: Mutex<HashMap<PathBuf, Head>>,
    /// The branch each session's agent last knew its worktree to be on.
    branches_known: Mutex<HashMap<i64, Head>>,
    /// The sub-agents each session's agent has at work in the background, by their task ids.
    background_agents: Mutex<HashMap<i64, HashSet<String>>>,
    /// The sessions in a turn a message of the user's started, each with whether that turn
    /// has started a sub-agent in the background.
    answering: Mutex<HashMap<i64, bool>>,
    /// The messages told to each session's agent in the middle of a turn and not yet heard:
    /// each is recorded where the agent says it took it in.
    told: Mutex<HashMap<i64, VecDeque<Told>>>,
    /// The latest messages sent to each session's agent from here, as sent, which it says back
    /// as it takes them in: one it says back that is none of these, and was not told, was
    /// typed elsewhere.
    sent: Mutex<HashMap<i64, VecDeque<String>>>,
    /// The sessions whose agent has Remote Control on, or was asked to turn it on, in the run
    /// it is in now.
    remote: Mutex<HashSet<i64>>,
    /// Who waits for each session's agent to answer whether work went to the background,
    /// first asked first.
    sending: Mutex<HashMap<i64, Vec<Sending>>>,
    /// The account's usage as any Claude Code session last heard it.
    pub(crate) usage: UsageHeard,
    runs: AtomicU64,
    bus: broadcast::Sender<SessionEvent>,
}

impl Default for Live {
    fn default() -> Self {
        Self {
            commands: Mutex::default(),
            endings: Mutex::default(),
            open_requests: Mutex::default(),
            restarts: Mutex::default(),
            queues: Queues::default(),
            pids: Mutex::default(),
            stacks: Mutex::default(),
            turns: Mutex::default(),
            branches_seen: Mutex::default(),
            branches_known: Mutex::default(),
            background_agents: Mutex::default(),
            answering: Mutex::default(),
            told: Mutex::default(),
            sent: Mutex::default(),
            remote: Mutex::default(),
            sending: Mutex::default(),
            usage: UsageHeard::default(),
            runs: AtomicU64::new(0),
            bus: broadcast::Sender::new(EVENT_BUFFER),
        }
    }
}

impl Live {
    /// Hands a message to the session's agent in the middle of its turn, to be recorded as
    /// the user typed it once the agent says it took it in.
    pub(crate) fn tell(&self, id: i64, message: Told) -> Result<()> {
        let sent = message.sent.clone();
        lock(&self.told).entry(id).or_default().push_back(message);
        let told = self.command(id, AgentCommand::UserMessage(sent));
        if told.is_err() {
            lock(&self.told).entry(id).or_default().pop_back();
        }
        told
    }

    /// What the user typed, if `text` is a message told to the session's agent mid-turn, which
    /// it has now heard. The earliest such message is taken: one told before it that the agent
    /// has not said back stays, and must not keep the later ones from being recorded.
    fn heard(&self, id: i64, text: &str) -> Option<String> {
        let mut told = lock(&self.told);
        let waiting = told.get_mut(&id)?;
        let at = waiting.iter().position(|message| message.sent == text)?;
        waiting.remove(at).map(|message| message.typed)
    }

    /// The messages told to the session's agent that it never said it heard, as the user
    /// typed them: its run is over, or another takes its place.
    fn never_heard(&self, id: i64) -> Vec<String> {
        let told = lock(&self.told).remove(&id).unwrap_or_default();
        told.into_iter().map(|message| message.typed).collect()
    }

    /// Notes a message sent to the session's agent from here, as it was sent. Only the latest
    /// few are kept: Claude Code says each back as it starts on it, which is soon, and some,
    /// such as commands, it never says back as they were sent.
    fn sent(&self, id: i64, message: &str) {
        let mut sent = lock(&self.sent);
        let kept = sent.entry(id).or_default();
        kept.push_back(message.to_string());
        while kept.len() > SENT_KEPT {
            kept.pop_front();
        }
    }

    /// Whether `text` is a message sent to the session's agent from here that it had not said
    /// back yet. It has now, and it is forgotten.
    fn was_sent(&self, id: i64, text: &str) -> bool {
        let mut sent = lock(&self.sent);
        let Some(kept) = sent.get_mut(&id) else {
            return false;
        };
        // Compared without the space around them, which Claude Code does not always keep.
        let at = kept
            .iter()
            .position(|message| message.trim() == text.trim());
        at.and_then(|at| kept.remove(at)).is_some()
    }

    /// Notes that someone waits for session `id`'s agent to answer whether the work of `call`,
    /// or of every call, went to the background, and is where the answer arrives.
    pub(crate) fn await_sent(&self, id: i64, call: Option<String>) -> oneshot::Receiver<Sent> {
        let (answer, answered) = oneshot::channel();
        let waiting = Sending { call, answer };
        lock(&self.sending).entry(id).or_default().push(waiting);
        answered
    }

    /// Hands the agent's answer about `call` to whoever asked about it first and still waits.
    /// One who gave up waiting is passed over.
    fn answer_sent(&self, id: i64, call: Option<&str>, answer: &Sent) {
        let mut sending = lock(&self.sending);
        let Some(waiting) = sending.get_mut(&id) else {
            return;
        };
        let mut given = false;
        while !given && let Some(at) = waiting.iter().position(|one| one.call.as_deref() == call) {
            given = waiting.remove(at).answer.send(answer.clone()).is_ok();
        }
        if waiting.is_empty() {
            sending.remove(&id);
        }
    }

    /// Forgets who waits for session `id`'s agent to answer about the background, unless it
    /// runs: an agent that is not running answers nobody, and they hear so at once.
    pub(crate) fn sending_ends_unless_running(&self, id: i64) {
        if !self.is_running(id) {
            lock(&self.sending).remove(&id);
        }
    }

    /// Notes that the session's agent has been asked to turn Remote Control on.
    pub(crate) fn remote_begins(&self, id: i64) {
        lock(&self.remote).insert(id);
    }

    /// Notes that the session's agent no longer has Remote Control on, and says whether it
    /// had.
    pub(crate) fn remote_ends(&self, id: i64) -> bool {
        lock(&self.remote).remove(&id)
    }

    /// Whether the session's agent has Remote Control on in the run it is in now.
    pub(crate) fn is_remote(&self, id: i64) -> bool {
        lock(&self.remote).contains(&id)
    }

    /// Notes that `worktree` is at `now`, and says where it was before if that was elsewhere.
    /// Nothing the first time: there is nothing to have moved from.
    pub(crate) fn branch_seen(&self, worktree: &Path, now: &Head) -> Option<Head> {
        let before = lock(&self.branches_seen).insert(worktree.to_path_buf(), now.clone())?;
        (before != *now).then_some(before)
    }

    /// Notes that the agent of session `id` knows its worktree to be at `now`, and says what it
    /// knew before if that was elsewhere. Nothing the first time.
    fn branch_known(&self, id: i64, now: &Head) -> Option<Head> {
        let before = lock(&self.branches_known).insert(id, now.clone())?;
        (before != *now).then_some(before)
    }

    fn commands(&self) -> MutexGuard<'_, Commands> {
        self.commands.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn sender(&self, id: i64) -> Result<mpsc::UnboundedSender<AgentCommand>> {
        self.commands()
            .get(&id)
            .map(|run| run.commands.clone())
            .ok_or(Error::SessionNotRunning(id))
    }

    pub(crate) fn is_running(&self, id: i64) -> bool {
        self.commands().contains_key(&id)
    }

    /// Stops the session's agent, if one runs. Dropping the run closes the agent's input,
    /// which asks it to end, and tells the task following the run to see that it does.
    fn stop(&self, id: i64) {
        // A restart that was still to come is settled by this.
        lock(&self.restarts).remove(&id);
        let stopped = self.commands().remove(&id);
        drop(stopped);
    }

    /// The session's runs that have not ended yet: the one running, and any that were
    /// stopped and are on their way out.
    fn endings_of(&self, id: i64) -> Vec<Ending> {
        let endings = lock(&self.endings);
        let of_session = endings.values().filter(|(session, _)| *session == id);
        of_session.map(|(_, ending)| ending.clone()).collect()
    }

    /// How what run number `run` of session `id` says is to be taken.
    fn standing(&self, id: i64, run: u64, ending: &Ending) -> Standing {
        if ending.discarded.load(Ordering::Relaxed) {
            return Standing::Discarded;
        }
        let commands = self.commands();
        let replaced = commands.get(&id).is_some_and(|live| live.number != run);
        if replaced {
            Standing::Replaced
        } else {
            Standing::Current
        }
    }

    /// Keeps and passes on what a run says after another has taken its place, without
    /// letting it change how the session stands: no status, no open requests, nothing to
    /// read. That its turn or its process ended is dropped altogether, since front ends take
    /// either for the end of the session's own turn, which is the new agent's to report. So is
    /// a message it says back: what it was told and never heard waits in the queue again.
    fn note_late(&self, store: &Mutex<Store>, session_id: i64, event: AgentEvent) {
        if dropped_when_late(&event) {
            return;
        }
        if !shown_only(&event) {
            lock(store).keep(session_id, &event).ok();
        }
        // What it heard of the account's usage is as true as what the run in its place hears.
        let usage = self.usage.hear(store, session_id, &event);
        // An error only means nobody is subscribed right now.
        self.bus
            .send(SessionEvent {
                session_id,
                status: None,
                unread: None,
                event,
            })
            .ok();
        self.publish_usage(usage);
    }

    pub(crate) fn command(&self, id: i64, command: AgentCommand) -> Result<()> {
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

    /// Tells every front end of something that belongs to no session, under session id 0.
    pub(crate) fn publish_apart(&self, event: AgentEvent) {
        let apart = SessionEvent {
            session_id: 0,
            status: None,
            unread: None,
            event,
        };
        // An error only means nobody is subscribed right now.
        self.bus.send(apart).ok();
    }

    /// Tells every front end the account's usage, if it changed.
    fn publish_usage(&self, usage: Option<AccountUsage>) {
        if let Some(usage) = usage {
            self.publish_apart(AgentEvent::AccountUsage { usage });
        }
    }

    /// Stores an event, unless it is one that is only ever shown, then publishes it, and
    /// after it the account's usage if the event changed that.
    pub(crate) fn note(
        &self,
        store: &Mutex<Store>,
        session_id: i64,
        event: AgentEvent,
    ) -> Result<()> {
        let event = self.timed(session_id, event);
        let status = self.status_after(session_id, &event);
        self.follow_background(session_id, &event);
        let unread = self.unread_after(session_id, &event);
        let stored = if shown_only(&event) {
            Ok(())
        } else {
            lock(store).record(session_id, &event, status, unread)
        };
        let usage = self.usage.hear(store, session_id, &event);
        // An error only means nobody is subscribed right now.
        self.bus
            .send(SessionEvent {
                session_id,
                status,
                unread,
                event,
            })
            .ok();
        self.publish_usage(usage);
        stored
    }

    /// Keeps which sub-agents each session's agent has at work in the background, from its
    /// reports of them: started and not yet ended. A command left running does not count, as
    /// a dev server may never end, and nothing outlives the agent.
    fn follow_background(&self, session_id: i64, event: &AgentEvent) {
        match event {
            AgentEvent::Background {
                task_id,
                kind,
                state,
                ..
            } => {
                // An end is reported without saying again what it was: any end takes it off.
                let running = *state == BackgroundState::Running;
                let starts = running && SUB_AGENT_KINDS.contains(&kind.as_str());
                self.note_background(session_id, task_id, starts);
            }
            AgentEvent::UserMessage { .. } => {
                lock(&self.answering).entry(session_id).or_insert(false);
            }
            AgentEvent::Exited { .. } => self.background_gone(session_id),
            _ => {}
        }
    }

    /// Notes that a sub-agent of a session's agent started in the background, or that
    /// something it left there ended.
    fn note_background(&self, session_id: i64, task_id: &str, starts: bool) {
        let mut working = lock(&self.background_agents);
        let ids = working.entry(session_id).or_default();
        if !starts {
            ids.remove(task_id);
            return;
        }
        ids.insert(task_id.to_string());
        if let Some(started) = lock(&self.answering).get_mut(&session_id) {
            *started = true;
        }
    }

    /// Forgets what a session's agent had at work in the background: a new run of it starts
    /// with nothing.
    fn background_gone(&self, session_id: i64) {
        lock(&self.background_agents).remove(&session_id);
        lock(&self.answering).remove(&session_id);
    }

    /// What `event` does to whether the session is unread. An answer handed back makes it
    /// unread, unless sub-agents the agent left working in the background are still at it: the
    /// session can take a message then, but its work is not done. That holds for the turn that
    /// started them and for turns of the agent's own; the answer to a later message of the
    /// user's is handed back whatever still works. A message of the user's makes it read.
    fn unread_after(&self, session_id: i64, event: &AgentEvent) -> Option<bool> {
        match event {
            AgentEvent::UserMessage { .. } => Some(false),
            _ if leaves_something_to_read(event) => {
                let waits = lock(&self.background_agents)
                    .get(&session_id)
                    .is_some_and(|ids| !ids.is_empty());
                // The turn is over: what it was is asked once.
                let answers = lock(&self.answering).remove(&session_id) == Some(false);
                (!waits || answers).then_some(true)
            }
            _ => None,
        }
    }

    /// Keeps the time of each turn: a message starts the clock, and the end of the turn is
    /// told how long it took. A turn that ends with its agent is forgotten.
    fn timed(&self, session_id: i64, event: AgentEvent) -> AgentEvent {
        let mut turns = lock(&self.turns);
        match event {
            AgentEvent::UserMessage { .. } => {
                turns.entry(session_id).or_insert_with(Instant::now);
                event
            }
            AgentEvent::TurnDone {
                is_error,
                result,
                cost_usd,
                duration_ms,
            } => {
                let took = turns
                    .remove(&session_id)
                    .map(|began| u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX));
                AgentEvent::TurnDone {
                    is_error,
                    result,
                    cost_usd,
                    duration_ms: duration_ms.or(took),
                }
            }
            AgentEvent::Exited { .. } => {
                turns.remove(&session_id);
                event
            }
            _ => event,
        }
    }

    /// Stops the session's agent so it starts again with new settings: at once if it is
    /// between turns, else when its turn is over. Does nothing if it is not running.
    fn restart_when_idle(&self, id: i64, status: SessionStatus) {
        let mut commands = self.commands();
        if !commands.contains_key(&id) {
            return;
        }
        if matches!(status, SessionStatus::Working | SessionStatus::Waiting) {
            lock(&self.restarts).insert(id);
        } else {
            commands.remove(&id);
        }
    }

    /// Forgets a run whose agent has gone. The session no longer counts as running, unless
    /// it was started again in the meantime.
    fn forget(&self, id: i64, run: u64) {
        lock(&self.endings).remove(&run);
        let mut commands = self.commands();
        if commands.get(&id).is_some_and(|live| live.number == run) {
            commands.remove(&id);
        }
        drop(commands);
        self.sending_ends_unless_running(id);
    }
}

/// Records and publishes the views the session's tool server was given since last time.
fn show_views(store: &Mutex<Store>, live: &Live, id: i64, spool: &Path) {
    for html in take_views(spool) {
        live.note(store, id, AgentEvent::View { html }).ok();
    }
}

/// Records a move of the task's worktree to another branch, seen when a tool answered or the
/// turn ended: once per move, in the session whose agent was at work, which knows of it from
/// then on. The branch is remembered for the task even if nobody looks at its panel meanwhile.
fn notice_moved(shared: &Shared, id: i64) {
    let worktree = &shared.spec.cwd;
    let Some(now) = git::head_now(worktree) else {
        return;
    };
    let Some(before) = shared.live.branch_seen(worktree, &now) else {
        return;
    };
    shared.live.branch_known(id, &now);
    if let Some(branch) = &now.0 {
        remember_branch(&shared.store, id, branch);
    }
    let event = AgentEvent::WorktreeMoved {
        from: before.0,
        to: now.0,
        between_turns: false,
    };
    shared.live.note(&shared.store, id, event).ok();
}

/// Remembers `branch` among the branches of session `id`'s task, unless it is the task's own.
fn remember_branch(store: &Mutex<Store>, id: i64, branch: &str) {
    let store = lock(store);
    let task = store.session_by_id(id).ok().map(|session| session.task_id);
    let task = task.and_then(|task| store.task_by_id(task).ok().flatten());
    if let Some(task) = task.filter(|task| task.branch != branch) {
        store.task_branch_seen(task.id, branch).ok();
    }
}

/// What the task following one agent's events needs of the core.
#[derive(Clone)]
struct Shared {
    store: Arc<Mutex<Store>>,
    live: Arc<Live>,
    terminals: Arc<Terminals>,
    /// Where skills are found, for a waiting message that calls one.
    paths: Paths,
    /// Where the session's tool server leaves the views it is given.
    spool: PathBuf,
    /// What runs the session's agent, and what it was started with: for starting it again
    /// from here, when new settings are due and a message is waiting for them.
    backend: Arc<dyn AgentBackend>,
    spec: SessionSpec,
}

impl Shared {
    /// What sending from the session's queue needs.
    fn outbox(&self) -> queue::Outbox {
        queue::Outbox {
            store: Arc::clone(&self.store),
            live: Arc::clone(&self.live),
            terminals: Arc::clone(&self.terminals),
            paths: self.paths.clone(),
        }
    }
}

/// Whether an agent is started only for a session that has none running, or in place of the
/// one it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Launch {
    UnlessRunning,
    Replacing,
}

/// Starts the agent `shared` describes for session `id`, and a task that follows it to its
/// end. Must be called from within a tokio runtime.
fn launch(shared: Shared, id: i64, how: Launch) -> Result<()> {
    let live = Arc::clone(&shared.live);
    // Held from the check to the insert, so two callers cannot start the same session twice.
    // Only the start itself happens under it: what it needs was worked out before.
    let mut running = live.commands();
    if how == Launch::UnlessRunning {
        if running.contains_key(&id) {
            return Ok(());
        }
        // Whatever an earlier run left behind belongs to a conversation that has moved on.
        std::fs::remove_dir_all(&shared.spool).ok();
    }
    let handle = shared.backend.start(shared.spec.clone())?;
    live.background_gone(id);
    // A new process has heard nothing yet and has nothing turned on.
    lock(&live.sent).remove(&id);
    live.remote_ends(id);
    // What a run this takes the place of was told and never heard: its own end is dropped.
    let unheard = live.never_heard(id);
    let number = live.runs.fetch_add(1, Ordering::Relaxed);
    let (stop, stopped) = oneshot::channel();
    let (gone, ended) = watch::channel(());
    let ending = Ending {
        tasks: handle.tasks,
        gone: ended,
        discarded: Arc::default(),
    };
    // A run this takes the place of is dropped here, which stops it.
    let run = Run {
        number,
        commands: handle.commands,
        _stop: stop,
    };
    running.insert(id, run);
    drop(running);
    queue::wait_again(&shared.store, &live, id, &unheard);
    lock(&live.endings).insert(number, (id, ending.clone()));
    if let Some(pid) = handle.pid {
        lock(&live.pids).insert(id, pid);
    }
    remote::resume(&shared.store, &live, id);
    let reaper = Reaper {
        stopped,
        kill_at: None,
        ending,
        _gone: gone,
    };
    drop(tokio::spawn(record_and_publish(
        handle.events,
        shared,
        (id, number),
        reaper,
    )));
    Ok(())
}

/// Starts the session's agent again, in place of the one running, with the settings the
/// session has now. Everything else it is started with stays as it was.
fn relaunch(shared: &Shared, id: i64) -> Result<()> {
    let session = lock(&shared.store).session_by_id(id)?;
    let mut next = shared.clone();
    next.spec.model = session.model;
    next.spec.supervision = session.supervision;
    next.spec.effort = session.effort;
    next.spec.fast = session.fast;
    next.spec.resume = session.agent_session_id;
    launch(next, id, Launch::Replacing)
}

/// Follows one run to its end: hands out its events, and sees that an agent that was stopped
/// goes, by killing it if it has not gone by itself after [`GRACE`].
struct Reaper {
    /// Resolves when the run is dropped from the running ones: stopped, or replaced.
    stopped: oneshot::Receiver<()>,
    /// When the agent is killed, once it was stopped.
    kill_at: Option<tokio::time::Instant>,
    ending: Ending,
    /// Dropped with this, which tells whoever waits for the run's end that it has come.
    _gone: watch::Sender<()>,
}

impl Reaper {
    /// The run's next event, or `None` when its agent has gone or was killed.
    async fn next(
        &mut self,
        events: &mut mpsc::UnboundedReceiver<AgentEvent>,
    ) -> Option<AgentEvent> {
        loop {
            let kill_at = self.kill_at;
            let deadline = kill_at.unwrap_or_else(tokio::time::Instant::now);
            tokio::select! {
                event = events.recv() => return event,
                _ = &mut self.stopped, if kill_at.is_none() => {
                    self.kill_at = Some(tokio::time::Instant::now() + GRACE);
                }
                () = tokio::time::sleep_until(deadline), if kill_at.is_some() => {
                    self.ending.kill();
                    return None;
                }
            }
        }
    }
}

/// Stores and publishes a run's events until its agent has gone.
///
/// Takes only the event half of the agent's handle: holding a command sender here would keep
/// the agent's input open, and [`Core::session_stop`] could never end it.
async fn record_and_publish(
    mut events: mpsc::UnboundedReceiver<AgentEvent>,
    shared: Shared,
    (id, run): (i64, u64),
    mut reaper: Reaper,
) {
    let mut exited = false;
    while let Some(event) = reaper.next(&mut events).await {
        exited |= matches!(event, AgentEvent::Exited { .. });
        take(&shared, (id, run), &reaper.ending, event);
    }
    if !exited {
        // Killed, or gone without a word: that it has ended is said for it.
        let ended = AgentEvent::Exited {
            code: None,
            stderr: String::new(),
        };
        take(&shared, (id, run), &reaper.ending, ended);
    }
    shared.live.forget(id, run);
}

/// Whether an event of a run that another has taken the place of changes nothing and is
/// dropped: its end, and what it says back. What it reports of Remote Control too, which is
/// about a connection that went with it: kept, it would stand as the session's latest word
/// on the one the run in its place has.
fn dropped_when_late(event: &AgentEvent) -> bool {
    matches!(
        event,
        AgentEvent::TurnDone { .. }
            | AgentEvent::Exited { .. }
            | AgentEvent::Heard { .. }
            | AgentEvent::RemoteControl { .. }
    )
}

/// Deals with one event of run number `run` of session `id`, by how that run stands now. An
/// answer to sending work to the background goes to whoever asked, whichever run gave it, and
/// nowhere else: the move itself is reported apart.
fn take(shared: &Shared, (id, run): (i64, u64), ending: &Ending, event: AgentEvent) {
    if let AgentEvent::SentToBackground {
        tool_use_id,
        moved,
        refused,
    } = event
    {
        let answer = (moved, refused);
        shared.live.answer_sent(id, tool_use_id.as_deref(), &answer);
        return;
    }
    match shared.live.standing(id, run, ending) {
        Standing::Current => on_event(shared, id, event),
        Standing::Replaced => shared.live.note_late(&shared.store, id, event),
        // Nothing is left to store it in, and front ends have been told the session is gone.
        Standing::Discarded => {}
    }
}

/// What an event of the agent's is once messages told to it mid-turn are accounted for: a
/// message it says it took in is recorded as the user's, there, as they typed it; any other it
/// says back is dropped, since it was recorded when it was sent; and at the agent's end what it
/// never heard waits in the queue again, so that nothing the user wrote goes missing or is
/// shown as said to an agent that did not get it.
fn heard_as(shared: &Shared, id: i64, event: AgentEvent) -> Option<AgentEvent> {
    let live = &shared.live;
    match event {
        AgentEvent::Heard { text } => {
            if let Some(typed) = live.heard(id, &text) {
                return Some(AgentEvent::UserMessage {
                    text: typed,
                    remote: false,
                });
            }
            // Sent from here, and recorded then. What is neither came through Remote Control,
            // typed on claude.ai or in the Claude app: the user's words, which nothing else
            // records.
            let afar = !live.was_sent(id, &text)
                && live.is_remote(id)
                && !remote::is_command_record(&text);
            afar.then_some(AgentEvent::UserMessage { text, remote: true })
        }
        AgentEvent::Exited { .. } => {
            let unheard = live.never_heard(id);
            queue::wait_again(&shared.store, live, id, &unheard);
            lock(&live.sent).remove(&id);
            Some(event)
        }
        _ => Some(event),
    }
}

/// Stores and publishes an event of the session's agent, and does what follows from it.
fn on_event(shared: &Shared, id: i64, event: AgentEvent) {
    let (store, live) = (&shared.store, &shared.live);
    let event = heard_as(shared, id, event);
    let Some(event) = event.and_then(|event| remote::reported(store, live, id, event)) else {
        return;
    };
    let turn_over = matches!(event, AgentEvent::TurnDone { .. });
    let gone = matches!(event, AgentEvent::Exited { .. });
    // A turn that ended well makes room for the next message the user left waiting, and so
    // does one the user stopped (marked by `session_interrupt`). One that failed does not:
    // what comes next is then theirs to decide.
    let finished = matches!(
        event,
        AgentEvent::TurnDone {
            is_error: false,
            ..
        }
    );
    // A tool has answered or the turn is over: time to show what the agent handed to
    // Swarmed's own tool meanwhile, however it came to call it.
    let settled = matches!(
        &event,
        AgentEvent::ToolResult { .. } | AgentEvent::TurnDone { .. }
    ) || matches!(&event, AgentEvent::Sub { event, .. } if matches!(**event, AgentEvent::ToolResult { .. }));
    // A full disk must not cut the user off from a running agent: the event is published
    // even if storing it failed.
    live.note(store, id, event).ok();
    if settled {
        show_views(store, live, id, &shared.spool);
        notice_moved(shared, id);
    }
    if turn_over {
        end_turn(shared, id, finished);
    }
    if gone {
        // An agent that went in the middle of a turn the user stopped never ends it.
        live.queues.pressing().remove(&id);
        remote::paused(store, live, id);
    }
}

/// What follows the end of a turn. The first waiting message goes if the turn ended well or
/// the user stopped it ([`Core::session_interrupt`], [`Core::session_send_now`]), unless a
/// message went to the agent since; an agent whose settings changed during the turn is
/// started again first, or once the turn of a message that went since has ended.
fn end_turn(shared: &Shared, id: i64, finished: bool) {
    let (store, live) = (&shared.store, &shared.live);
    // Held until the message has gone: one sent from the queue at once right as the turn
    // ended is then either sent here or already with the agent, never both.
    let mut pressing = live.queues.pressing();
    let pressed = pressing.remove(&id);
    let busy = lock(store)
        .session_by_id(id)
        .is_ok_and(|session| is_busy_status(session.status));
    let sends = (finished || pressed) && !busy;
    // Restarting now would end the turn of a message that went since.
    if !busy && lock(&live.restarts).remove(&id) {
        restart(shared, id, sends);
    }
    if sends {
        queue::send_next(&shared.outbox(), id).ok();
    }
}

/// Stops the agent of a session whose settings changed during the turn that is now over, so
/// that it starts again with them. With nothing to send, it starts with the next message. A
/// message waiting in the queue that is sent as this turn ends is to be answered with the
/// new settings the user wants: the agent is started again here, before the message goes.
fn restart(shared: &Shared, id: i64, sends: bool) {
    let waiting = sends && shared.live.queues.is_waiting(id);
    if !(waiting && relaunch(shared, id).is_ok()) {
        shared.live.stop(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::RemoteState;

    #[test]
    fn a_replaced_run_has_no_say_on_remote_control_but_its_words_are_kept() {
        let failed = AgentEvent::RemoteControl {
            state: RemoteState::Failed,
            url: None,
            remote_id: None,
            detail: Some("the bridge closed".into()),
        };
        assert!(dropped_when_late(&failed));
        assert!(dropped_when_late(&AgentEvent::Heard { text: "hi".into() }));
        assert!(!dropped_when_late(&AgentEvent::Text { text: "hi".into() }));
    }

    #[test]
    fn a_message_said_back_is_known_as_sent_once_whatever_space_is_around_it() {
        let live = Live::default();
        live.sent(7, "hello\n");
        assert!(!live.was_sent(7, "something typed elsewhere"));
        assert!(!live.was_sent(8, "hello"));
        assert!(live.was_sent(7, "hello"));
        // Said back, it is forgotten: the same words again were typed elsewhere.
        assert!(!live.was_sent(7, "hello"));
    }

    #[test]
    fn a_message_heard_is_recorded_as_typed_whatever_was_told_before_it() {
        let live = Live::default();
        let (commands, _agent) = mpsc::unbounded_channel();
        let (stop, _stopped) = oneshot::channel();
        let run = Run {
            number: 1,
            commands,
            _stop: stop,
        };
        live.commands().insert(7, run);
        let told = |sent: &str, typed: &str| Told {
            sent: sent.to_string(),
            typed: typed.to_string(),
        };
        live.tell(7, told("never said back", "/lost"))
            .expect("tells");
        live.tell(7, told("what the skill says", "/skill"))
            .expect("tells");
        // One the agent never says back does not keep the next from being recorded.
        assert_eq!(live.heard(7, "something else"), None);
        assert_eq!(
            live.heard(7, "what the skill says"),
            Some("/skill".to_string())
        );
        assert_eq!(live.heard(7, "what the skill says"), None);
        assert_eq!(live.never_heard(7), ["/lost"]);
        assert_eq!(live.never_heard(7), Vec::<String>::new());
    }

    #[test]
    fn a_title_is_taken_from_what_was_typed_and_not_from_what_was_pasted() {
        let paste = "<pasted_text_1>\nBuild ok\nTests ok\n</pasted_text_1>";
        assert_eq!(
            title_from(&format!("{paste}\nsummarise this report")),
            "summarise this report"
        );
        assert_eq!(
            title_from(&format!("look at {paste} please")),
            "look at  please"
        );
        assert_eq!(title_from(paste), "Pasted text");
        // A tag that is only mentioned is text like any other.
        assert_eq!(
            title_from("what is <pasted_text_1> for?"),
            "what is <pasted_text_1> for?"
        );
    }

    #[test]
    fn title_is_the_first_line_shortened() {
        assert_eq!(title_from("  fix the login\nmore detail"), "fix the login");
        let long = "x".repeat(200);
        let title = title_from(&long);
        assert_eq!(title.chars().count(), TITLE_CHARS);
        assert!(title.ends_with('…'));
    }

    #[test]
    fn a_suggested_title_loses_its_quotes_and_extra_lines() {
        assert_eq!(
            shortened("  \"Fix the login loop\"\n\nSure!", 60),
            "Fix the login loop"
        );
        assert_eq!(shortened("   ", 60), "");
    }

    #[test]
    fn what_a_sub_agent_does_leaves_the_status_as_it_is() {
        let said = AgentEvent::Sub {
            parent: "tool-1".to_string(),
            event: Box::new(AgentEvent::Text {
                text: "looking".to_string(),
            }),
        };
        assert_eq!(SessionStatus::after(&said, false), None);
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

//! What sessions can do about each other: see who else is at work, read what another has said,
//! leave it a message, start a new session beside themselves, and start a new task for work
//! that needs a branch of its own (`peers/tasks.rs`).
//!
//! Agents reach this through Swarmed's own tool server. That server is a process the agent
//! starts, so it asks the core over a socket in the data directory: one request on a line, one
//! answer on a line. Everything here is also plain methods on [`Core`], which is what the
//! tests use.

use std::ops::RangeInclusive;
use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::agent::{AgentEvent, Auth, Supervision};
use crate::chats::CHATS;
use crate::cloud::{CloudOrigin, CloudProvider};
use crate::error::{Error, Result};
use crate::paths::SOCKET_PREFIX;
use crate::roles::Role;
use crate::sessions::{Session, SessionOptions, SessionStatus};
use crate::store::Project;
use crate::toolserver::{DELEGATE_TO_CLOUD, SHOW_BROWSER, START_TASK};
use crate::{Core, Task, parents, rewind, roles};

mod tasks;

pub use tasks::PeerTask;

/// How many open sessions may work for one session unless the user chose otherwise.
///
/// Beyond it its agent starts no more, beside it or in a task, so that a runaway agent stops
/// here instead of filling the machine. The user's choice is `Settings::max_workers`.
pub const MAX_SPAWNED: usize = 6;
/// How many open sessions the settings may let work for one session, at the fewest and the
/// most.
pub const WORKER_LIMITS: RangeInclusive<usize> = 1..=20;
/// How many sessions with a role may work one for another in a row: an architect, its
/// planners and their implementers. A chain of roles that start one another stops here.
pub const MAX_DEPTH: usize = 3;
/// How much of another session's conversation is handed over at most, in characters.
const DIGEST_CHARS: usize = 8000;
/// How many of a session's messages are read when no number is asked for.
const DIGEST_MESSAGES: usize = 12;

/// What agents are told about these tools, as part of Swarmed's guidance.
pub(crate) const INSTRUCTIONS: &str = "\
# Other sessions

You are one of possibly several agent sessions the user runs side by side in Swarmed, each in \
a task with its own git worktree. The `swarmed` tool server (in Claude Code: `mcp__swarmed__…`) \
lets you work with the others:
- `list_sessions`: who else is there, in which task, whether they are busy, and which session \
each works for.
- `read_session`: the latest messages of another session.
- `send_message`: leave another session a message. It arrives as a message in its \
conversation, at once if it is idle, else when its turn ends.
- `spawn_session`: start a new session in your own worktree with a prompt of yours, for a part \
of the work that can go on beside you. It shares your files, so give it a part that does not \
collide with what you are editing. It works for you, and is told to report back to you with \
`send_message` when it is done; you do not have to wait for it.
- `start_task`: start a new task, with a branch, worktree and session of its own, for work that \
needs a branch of its own: a separate change, a fix to another pull request, the next pull \
request of a stack. Its session gets your prompt once the task is set up, works for you, and \
reports back to you like a spawned one.
- `close_session`: put away a session you started with `spawn_session` or `start_task`, once it \
is done and you have what you need from it; a task stays as it is. You cannot close any other \
session.
- `delegate_to_cloud`: hand a self-contained piece of work to an agent in the cloud, when the \
user wants something delegated or run there. It works on what is pushed to GitHub, not on \
your worktree, and does not report back to you.
- `configure`: change Swarmed's own settings when the user asks you to: keep a prompt, add a \
button, add a rule, change the status line or the colours. Some changes are only proposed, \
and made when the user accepts them.
- `set_mark`: put a colour and a few words on your own session in the sidebar, when the user, \
a skill or a prompt asks for one, such as green once a check has passed. It stays until the \
user writes to you again.

Use these when they help the user's task, not as a matter of course. A session or task you \
start costs the user money like any other.";

/// How a message to another session was passed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// The session was idle and has started on it.
    Sent,
    /// The session is busy; the message waits in its queue until its turn ends.
    Queued,
}

impl Delivery {
    /// What the sender is told.
    fn said(self) -> &'static str {
        match self {
            Self::Sent => "Delivered: that session has started on it.",
            Self::Queued => {
                "That session is busy. Your message is in its queue and arrives when its turn ends."
            }
        }
    }
}

fn refused(reason: impl Into<String>) -> Error {
    Error::Peers(reason.into())
}

/// `text` as session `sender` leaves it in another conversation, marked with who wrote it.
/// The UI reads the mark back (`isFromSession` in `transcript.ts`): it shows the sender, and
/// ↑ does not bring the message back as the user's.
fn left_by(sender: &Session, text: &str) -> String {
    let title = if sender.title.is_empty() {
        "untitled"
    } else {
        &sender.title
    };
    let from = sender.id;
    format!(
        "Message from session {from} (\"{title}\"), another agent session of the same \
         user:\n\n{text}"
    )
}

fn is_busy(session: &Session) -> bool {
    matches!(
        session.status,
        SessionStatus::Working | SessionStatus::Waiting
    )
}

/// Which sessions a session can reach: those of its own repository, or from a chat every one,
/// since a chat is often about work in a repository.
///
/// A repository's session can write to a chat, so that one a chat wrote to can answer it, and
/// can neither list nor read one: a chat may hold what it read of another repository, and
/// would hand that on to every repository otherwise.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Reach {
    Project {
        project: i64,
        /// The home of the chats, once there is one.
        chats: Option<i64>,
    },
    All,
}

impl Reach {
    /// Whether a session of `task` is listed and can be read.
    pub(crate) fn reads(self, task: &Task) -> bool {
        match self {
            Self::Project { project, .. } => task.project_id == project,
            Self::All => true,
        }
    }

    /// Whether a session of `task` can be written to: what can be read, and the chats.
    fn writes(self, task: &Task) -> bool {
        match self {
            Self::Project { chats, .. } => self.reads(task) || Some(task.project_id) == chats,
            Self::All => true,
        }
    }

    fn covers(self, task: &Task, what: Access) -> bool {
        match what {
            Access::Read => self.reads(task),
            Access::Write => self.writes(task),
        }
    }
}

/// What a session wants of another.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Access {
    Read,
    Write,
}

/// What a session asks to start.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Starting {
    /// A session beside itself, in its own worktree (`spawn_session`).
    Session,
    /// A task, with a session of its own (`start_task`).
    Task,
}

impl Starting {
    /// Why a session with no role that another session's agent started may not start this.
    fn refused_to_spawned(self) -> &'static str {
        match self {
            Self::Session => {
                "a session that was started by another session cannot start sessions itself"
            }
            Self::Task => {
                "a session that was started by another session cannot start tasks; the session \
                 that started it can"
            }
        }
    }
}

/// Where a session works, as another session is told of it: its chat, or its task, with the
/// repository where sessions of every repository are listed.
fn place(task: &Task, reach: Reach, projects: &[Project]) -> String {
    let project = projects
        .iter()
        .find(|project| project.id == task.project_id);
    match project {
        Some(project) if project.chats => format!("chat {}", task.name),
        Some(project) if matches!(reach, Reach::All) => {
            format!("task {}/{}", project.name, task.name)
        }
        _ => format!("task {}", task.name),
    }
}

/// One line about a session, as another session is told of it: its number, its state, where
/// it works, the session it works for, its role, and its title.
fn brief(session: &Session, place: &str, own: i64) -> String {
    let you = if session.id == own { " (you)" } else { "" };
    let state = if session.unread && !is_busy(session) {
        "finished, not yet read by the user"
    } else {
        session.status.as_str()
    };
    let title = if session.title.is_empty() {
        "untitled"
    } else {
        &session.title
    };
    let works_for = match session.parent {
        Some(parent) if parent == own => "you".to_string(),
        Some(parent) => parent.to_string(),
        None => "-".to_string(),
    };
    let role = session.role.as_deref().unwrap_or("-");
    format!(
        "{}{you}\t{state}\t{place}\t{works_for}\t{role}\t{title}",
        session.id
    )
}

/// What a session said or was told, if the event is that, as one paragraph.
fn spoken(event: &AgentEvent) -> Option<String> {
    match event {
        AgentEvent::UserMessage { text, .. } => Some(format!("User: {text}")),
        AgentEvent::Text { text } => Some(format!("Agent: {text}")),
        _ => None,
    }
}

/// The end of `text`, at most `limit` characters of it, cut at a character boundary.
fn tail(text: &str, limit: usize) -> &str {
    let skip = text.chars().count().saturating_sub(limit);
    text.char_indices()
        .nth(skip)
        .map_or("", |(at, _)| &text[at..])
}

/// The start of `text`, at most `limit` characters of it, cut at a character boundary.
fn head(text: &str, limit: usize) -> &str {
    text.char_indices()
        .nth(limit)
        .map_or(text, |(at, _)| &text[..at])
}

/// Some of what was said in a session, for another session to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Said {
    /// The number of the first message given and of the last, counting from 1.
    pub(crate) first: usize,
    pub(crate) last: usize,
    /// How many messages the session has.
    pub(crate) total: usize,
    /// The messages, each under who said it, at most as many characters as were asked for.
    pub(crate) text: String,
}

/// Which messages of `said` are read: `wanted` of them, the latest unless `first` names the one
/// to start at, counting from 1. The text is cut to `chars` at the end that matters least: the
/// start of the latest messages, the end of those read from a start.
fn read_of(said: &[String], first: Option<usize>, wanted: usize, chars: usize) -> Option<Said> {
    let total = said.len();
    let wanted = wanted.max(1);
    let start = match first {
        None => total.saturating_sub(wanted),
        Some(first) => first.max(1) - 1,
    };
    if start >= total {
        return None;
    }
    let end = (start + wanted).min(total);
    let joined = said[start..end].join("\n\n");
    let text = if first.is_some() {
        head(&joined, chars)
    } else {
        tail(&joined, chars)
    };
    Some(Said {
        first: start + 1,
        last: end,
        total,
        text: text.to_string(),
    })
}

/// What a reader is told when there is nothing to read from message `first` on.
fn nothing_from(first: Option<usize>) -> String {
    match first {
        Some(first) if first > 1 => format!("That session has fewer than {first} messages."),
        _ => "Nothing has been said in that session yet.".to_string(),
    }
}

/// A session another session can point its agent at, as a front end lists it: an open one
/// the other can read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reachable {
    /// The session's id.
    pub id: i64,
    /// Its title; empty while it has none.
    pub title: String,
    /// The task it works in; for a chat, the chat's name.
    pub task: String,
    /// The repository of its task; for a chat, the home of the chats.
    pub project: String,
    /// Whether it is a chat.
    pub chat: bool,
    /// Whether it works in the same task as the session that would point at it.
    pub same_task: bool,
    /// What it is doing.
    pub status: SessionStatus,
}

impl Core {
    /// The task a session runs in.
    pub(crate) fn task_of(&self, session: &Session) -> Result<Task> {
        let task = self.store().task_by_id(session.task_id)?;
        task.ok_or(Error::SessionNotFound(session.id))
    }

    /// Which sessions session `own` can reach.
    pub(crate) fn reach_of(&self, own: &Session) -> Result<Reach> {
        self.reach_of_task(&self.task_of(own)?)
    }

    /// Which sessions a session of `task` can reach.
    fn reach_of_task(&self, task: &Task) -> Result<Reach> {
        if self.is_chat(task) {
            return Ok(Reach::All);
        }
        let chats = self.store().project_by_name(CHATS)?;
        Ok(Reach::Project {
            project: task.project_id,
            chats: chats.map(|home| home.id),
        })
    }

    /// Which sessions a session of a task of `project` would reach, before there is one. The
    /// home of the chats need not be there yet: it is made with the first chat.
    pub(crate) fn reach_of_project(&self, project: &str) -> Result<Reach> {
        if project == CHATS {
            return Ok(Reach::All);
        }
        let project = self.project(project)?;
        let chats = self.store().project_by_name(CHATS)?;
        Ok(Reach::Project {
            project: project.id,
            chats: chats.map(|home| home.id),
        })
    }

    /// The open sessions `from` can read, one line each, for an agent to read: those of the
    /// project it works in, or from a chat, every one, each with the session it works for.
    pub fn peer_sessions(&self, from: i64) -> Result<String> {
        let own = self.session(from)?;
        let reach = self.reach_of(&own)?;
        let (projects, tasks, sessions) = {
            let store = self.store();
            (store.projects()?, store.tasks_all()?, store.sessions_all()?)
        };
        let in_reach = |task: &&Task| reach.reads(task) && !task.archived;
        let line = |session: &Session| {
            let task = tasks.iter().find(|task| task.id == session.task_id);
            task.filter(in_reach)
                .map(|task| brief(session, &place(task, reach, &projects), from))
        };
        let mut lines = vec!["id\tstate\ttask\tworks for\trole\ttitle".to_string()];
        lines.extend(sessions.iter().filter_map(line));
        Ok(lines.join("\n"))
    }

    /// The sessions session `from` can point its agent at: every other one
    /// [`Core::peer_sessions`] lists, those of its own task first, each part latest active
    /// first. The reach rules are the agent's own: a front end lists what its tools can read.
    pub fn session_reachable(&self, from: i64) -> Result<Vec<Reachable>> {
        let own = self.session(from)?;
        let reach = self.reach_of(&own)?;
        self.reachable(reach, Some(&own))
    }

    /// The sessions a new task's first session in `project` can point its agent at, before
    /// there is one: those of the repository, or for a chat, every open session. Latest active
    /// first.
    pub fn project_reachable(&self, project: &str) -> Result<Vec<Reachable>> {
        let reach = self.reach_of_project(project)?;
        self.reachable(reach, None)
    }

    /// The open sessions `reach` covers, but `own`, those of its task first.
    fn reachable(&self, reach: Reach, own: Option<&Session>) -> Result<Vec<Reachable>> {
        let (projects, tasks, sessions, active) = {
            let store = self.store();
            let active = store.sessions_last_active()?;
            (
                store.projects()?,
                store.tasks_all()?,
                store.sessions_all()?,
                active,
            )
        };
        let listed = |session: Session| {
            let task = tasks.iter().find(|task| task.id == session.task_id)?;
            let project = projects.iter().find(|p| p.id == task.project_id)?;
            let shown = reach.reads(task) && !task.archived;
            let other = own.is_none_or(|own| own.id != session.id);
            (shown && other).then(|| Reachable {
                id: session.id,
                title: session.title,
                task: task.name.clone(),
                project: project.name.clone(),
                chat: project.chats,
                same_task: own.is_some_and(|own| own.task_id == task.id),
                status: session.status,
            })
        };
        let mut found: Vec<Reachable> = sessions.into_iter().filter_map(listed).collect();
        let latest = |session: &Reachable| active.get(&session.id).copied().unwrap_or(0);
        found.sort_by_key(|session| {
            std::cmp::Reverse((session.same_task, latest(session), session.id))
        });
        Ok(found)
    }

    /// Session `id` if `reach` covers it for `what`: an open session in a task that is not
    /// archived.
    pub(crate) fn in_reach(&self, reach: Reach, id: i64, what: Access) -> Option<Session> {
        let other = self.session(id).ok()?;
        let task = self.task_of(&other).ok()?;
        let closed = self.store().session_is_closed(id).ok()?;
        let listed = reach.covers(&task, what) && !task.archived && !closed;
        listed.then_some(other)
    }

    /// Session `id` as session `from` may reach it: an open session in a task of the project
    /// `from` works in, or from a chat any open session, which is what
    /// [`Core::peer_sessions`] lists; to write to, an open chat as well. Anything else is
    /// refused in the same words, so that asking tells an agent nothing about what it may not
    /// see.
    fn peer_in_reach(&self, from: i64, id: i64, what: Access) -> Result<Session> {
        let own = self.session(from)?;
        let reach = self.reach_of(&own)?;
        self.in_reach(reach, id, what).ok_or_else(|| {
            refused(format!(
                "there is no open session {id} you can reach; `list_sessions` says which there \
                 are"
            ))
        })
    }

    /// What was said in session `id`, by the user and by its agent: `wanted` messages, the
    /// latest unless `first` names the one to start at, at most `chars` characters. `None` if
    /// there is no such message, or nothing has been said. Tool calls and their output are
    /// left out: they are the session's workings, and would drown what it concluded.
    pub(crate) fn said_in(
        &self,
        id: i64,
        first: Option<usize>,
        wanted: usize,
        chars: usize,
    ) -> Result<Option<Said>> {
        let events = self.session_events(id)?;
        // Not what a rewind dropped: the session's agent no longer knows of it either.
        let said: Vec<String> = rewind::counted(&events).filter_map(spoken).collect();
        Ok(read_of(&said, first, wanted, chars))
    }

    /// What was said in session `id`, by the user and by its agent, oldest first, for session
    /// `from` to read: `messages` of them, the latest unless `first` names the one to start at,
    /// counting from 1, so that an agent can read a long conversation from its start or page
    /// back. Where that is not all of it, it is said which messages these are.
    ///
    /// Only an open session `from` can reach can be read: of its own project, or any from a
    /// chat. A repository's session cannot read a chat, which may hold another repository's.
    /// Its role and who works with it come first, if it has one or anyone does, so that an
    /// agent can find again the sessions that work for it.
    pub fn peer_digest(
        &self,
        from: i64,
        id: i64,
        messages: Option<usize>,
        first: Option<usize>,
    ) -> Result<String> {
        let read_from = self.peer_in_reach(from, id, Access::Read)?;
        let read = self.peer_read(id, messages, first)?;
        let relations = self.store().relations(id)?;
        Ok(match parents::about(&read_from, &relations) {
            Some(about) => format!("{about}\n\n{read}"),
            None => read,
        })
    }

    /// What was said in session `id`, as [`Core::peer_digest`] hands it on.
    fn peer_read(&self, id: i64, messages: Option<usize>, first: Option<usize>) -> Result<String> {
        let wanted = messages.unwrap_or(DIGEST_MESSAGES);
        let Some(said) = self.said_in(id, first, wanted, DIGEST_CHARS)? else {
            return Ok(nothing_from(first));
        };
        let Said {
            first,
            last,
            total,
            text,
        } = said;
        if first == 1 && last == total {
            return Ok(text);
        }
        let more = if last < total {
            format!("; read on with `from` {}", last + 1)
        } else {
            String::new()
        };
        let earlier = if first > 1 {
            "; earlier ones are read with `from`"
        } else {
            ""
        };
        Ok(format!(
            "(Messages {first} to {last} of {total}{earlier}{more}.)\n\n{text}"
        ))
    }

    /// Leaves session `to` a message from session `from`. It arrives as a message in the
    /// conversation, marked with who sent it: now if `to` is idle, starting its agent if need
    /// be, else when its turn ends.
    ///
    /// Only an open session `from` can reach can be written to: one it can read, or a chat, so
    /// that a session a chat wrote to can answer it. A closed one
    /// stays closed: a message must not start an agent where no list shows it, nor bring back
    /// sessions that were closed to make room under `Settings::max_workers`.
    pub fn peer_message(&self, from: i64, to: i64, text: &str) -> Result<Delivery> {
        let sender = self.session(from)?;
        if from == to {
            return Err(refused("a session cannot send a message to itself"));
        }
        let receiver = self.peer_in_reach(from, to, Access::Write)?;
        if text.trim().is_empty() {
            return Err(Error::EmptyMessage);
        }
        let message = left_by(&sender, text);
        if is_busy(&receiver) {
            self.session_enqueue(to, &message)?;
            return Ok(Delivery::Queued);
        }
        self.session_start(to, Auth::Subscription)?;
        self.session_send(to, &message)?;
        Ok(Delivery::Sent)
    }

    /// Starts a new session beside `from`, in the same task and so the same worktree, on the
    /// same agent with the same settings, and gives it `prompt`. The new session works for
    /// `from`, and is told who started it and to report back when it is done.
    ///
    /// The new session has the role called `role`, or without one the role `from`'s own starts
    /// by default (`roles.rs`); a role `from`'s may not start is refused (`may_start`).
    /// It starts as `from` would without its role, under the supervision its own role holds if
    /// it holds one: an implementer a planner starts is not held in the planner's plan mode.
    /// Refused while as many open sessions work for `from` as the settings allow
    /// (`room_for_another`).
    pub fn peer_spawn(
        &self,
        from: i64,
        prompt: &str,
        title: Option<&str>,
        role: Option<&str>,
    ) -> Result<Session> {
        let parent = self.session(from)?;
        if prompt.trim().is_empty() {
            return Err(Error::EmptyMessage);
        }
        let role = self.may_start(&parent, role, Starting::Session)?;
        self.room_for_another(from)?;
        let options = SessionOptions {
            agent: Some(parent.agent.clone()),
            title: title
                .map(String::from)
                .filter(|title| !title.trim().is_empty()),
            model: parent.model.clone(),
            supervision: self.supervision_without_role(&parent)?,
            effort: parent.effort,
            fast: parent.fast,
            role: role.map(|role| role.name.to_string()),
        };
        let child = self.session_insert(parent.task_id, &options)?;
        self.store().session_set_spawned_by(child.id, from)?;
        let message = format!(
            "{prompt}\n\n(You were started by session {from}, another agent session of the same \
             user, working in this same worktree. When you are done, report what you did and \
             found to it with the `send_message` tool, to session {from}.)"
        );
        self.session_start(child.id, Auth::Subscription)?;
        // As typed: a prompt that begins like a command is the starting agent's words, never
        // a command of the user's, which could turn Remote Control on with nobody asked.
        self.session_send_as_typed(child.id, &message)?;
        self.session(child.id)
    }

    /// Refuses while as many open sessions work for session `from` as the user's settings
    /// allow (`Settings::max_workers`, [`MAX_SPAWNED`] unless changed), however each came to:
    /// one more that its agent starts, beside it or in a task, would be one too many. The
    /// refusal names the setting, so that the agent can tell the user what to change.
    pub(crate) fn room_for_another(&self, from: i64) -> Result<()> {
        let open = self.store().sessions_spawned_by(from)?;
        let most = self.settings()?.max_workers;
        if open >= most {
            return Err(refused(format!(
                "this session already has {open} open sessions working for it, the most allowed: \
                 the user's settings let {most} work for one session (Workers per session, in \
                 Settings, Sessions; `max_workers` in settings.toml). Close those you started \
                 that are done, with close_session, or ask the user to close some or to allow \
                 more there"
            )));
        }
        Ok(())
    }

    /// The role of a session `from` starts, asking for `asked`, if it may start one: with a
    /// role, one its role starts ([`roles::of_child`]), while fewer than [`MAX_DEPTH`] sessions
    /// with a role work one for another in a row up from `from`; with none, any role, unless
    /// another session's agent started `from`, which then starts nothing. `None` for no role.
    pub(crate) fn may_start(
        &self,
        from: &Session,
        asked: Option<&str>,
        what: Starting,
    ) -> Result<Option<&'static Role>> {
        let own = from.role.as_deref().and_then(roles::find);
        if own.is_none() && from.spawned {
            return Err(refused(what.refused_to_spawned()));
        }
        let child = roles::of_child(from, asked)?;
        let in_a_row = self.roles_in_a_row(from);
        if own.is_some() && in_a_row >= MAX_DEPTH {
            return Err(refused(format!(
                "sessions with a role may work one for another at most {MAX_DEPTH} in a row, and \
                 this one is already the last of {in_a_row}; ask the session you work for, or \
                 the user, how to go on"
            )));
        }
        Ok(child)
    }

    /// How many sessions with a role work one for another in a row up from `session`, itself
    /// included: none if it has no role. Counted no further than one past [`MAX_DEPTH`].
    fn roles_in_a_row(&self, session: &Session) -> usize {
        let up = std::iter::successors(Some(session.clone()), |at| {
            at.parent.and_then(|parent| self.session(parent).ok())
        });
        up.take(MAX_DEPTH + 1)
            .take_while(|at| at.role.as_deref().and_then(roles::find).is_some())
            .count()
    }

    /// The supervision `session` has, or had before its role took its place: what a session
    /// it starts is asked to start in, before its own role has its say.
    pub(crate) fn supervision_without_role(&self, session: &Session) -> Result<Supervision> {
        let before = self.store().session_supervision_before_role(session.id)?;
        Ok(before.unwrap_or(session.supervision))
    }

    /// Closes session `id` on behalf of session `from`: one that works for `from` and that an
    /// agent started, and only once it is idle. That is one an agent started beside itself
    /// ([`Session::spawned`]), whichever agent did, so one the user moved to work for `from` is
    /// `from`'s to close too; or one `from`'s own agent started otherwise, with `start_task`
    /// or from its shell (`started_by`). A session the user started, even one the user has
    /// work for `from`, or one that works for another, is not an agent's to put away. Closing
    /// one in the middle of a turn would throw its work away, and closing one whose task is
    /// still set up, or whose first prompt still waits for that, would drop the prompt.
    ///
    /// Only the session is closed: its task, worktree, branch and pull request stay.
    pub fn peer_close(&self, from: i64, id: i64) -> Result<()> {
        self.session(from)?;
        // Whether the setup runs is asked before the session is read: a setup hands the first
        // prompt on before it ends, so one seen as over means the row read after it already
        // shows the turn, or the prompt still waiting. Read the other way round, a setup that
        // ended in between would pass a row from before the prompt went.
        let setting_up = self.setup_runs(&self.task_of(&self.session(id)?)?);
        let session = self.session(id)?;
        let started = session.spawned || self.store().session_started_by(id)? == Some(from);
        if session.parent != Some(from) || !started {
            return Err(refused(
                "a session can only close the sessions it started, with spawn_session or \
                 start_task, that work for it; this one is not yours",
            ));
        }
        if is_busy(&session) {
            return Err(refused(
                "that session is still at work; wait until it is done before closing it",
            ));
        }
        if setting_up {
            return Err(refused(
                "that session's task is still being set up; wait until it is done before \
                 closing it",
            ));
        }
        if session.waits_for_setup {
            return Err(refused(
                "that session's first prompt still waits for its task's setup, which did not \
                 finish well, and closing it would drop that prompt; ask the user to set the \
                 task up again or send the prompt anyway",
            ));
        }
        self.session_close(id)
    }

    /// Answers one request of the tool server: `tool` called by session `from` with
    /// `arguments`. The answer is text for the agent; a refusal is an error, also as text.
    /// Hands a part of the work of session `from` to a cloud agent, on the repository its
    /// task belongs to, and says where that went.
    async fn peer_delegate(&self, from: i64, arguments: &Value) -> Result<String> {
        let prompt = arguments.get("prompt").and_then(Value::as_str);
        let session = self.session(from)?;
        let task = self
            .store()
            .task_by_id(session.task_id)?
            .ok_or(Error::SessionNotFound(from))?;
        let project = self
            .store()
            .project_by_id(task.project_id)?
            .ok_or(Error::SessionNotFound(from))?;
        let named = arguments.get("provider").and_then(Value::as_str);
        let provider = match named {
            None => CloudProvider::default(),
            Some(name) => CloudProvider::parse(name)
                .ok_or_else(|| refused(format!("there is no cloud provider `{name}`")))?,
        };
        let origin = CloudOrigin {
            task: Some(&task.name),
            session: Some(from),
            provider,
        };
        let delegated = self
            .cloud_delegate(&project.name, prompt.unwrap_or_default(), origin)
            .await?;
        Ok(format!(
            "Delegated to a {} cloud agent, starting from the branch `{}` as it is on GitHub: \
             {}. It works on its own copy and does not report back here; the user follows it \
             at that address.",
            delegated.provider, delegated.branch, delegated.url
        ))
    }

    fn peer_call(&self, from: i64, tool: &str, arguments: &Value) -> Result<String> {
        let number = |key: &str| arguments.get(key).and_then(Value::as_i64);
        let word = |key: &str| arguments.get(key).and_then(Value::as_str);
        let other = || number("session_id").ok_or_else(|| refused("`session_id` is missing"));
        match tool {
            "list_sessions" => self.peer_sessions(from),
            "read_session" => {
                let count = |key: &str| number(key).and_then(|n| usize::try_from(n).ok());
                self.peer_digest(from, other()?, count("messages"), count("from"))
            }
            "send_message" => {
                let text = word("text").unwrap_or_default();
                let delivery = self.peer_message(from, other()?, text)?;
                Ok(delivery.said().to_string())
            }
            "spawn_session" => {
                let prompt = word("prompt").unwrap_or_default();
                let role = word("role").filter(|role| !role.trim().is_empty());
                let session = self.peer_spawn(from, prompt, word("title"), role)?;
                let what = session
                    .role
                    .map_or_else(String::new, |role| format!(" (role: {role})"));
                Ok(format!(
                    "Started session {}{what}. It works beside you in this worktree and will \
                     report back with a message when it is done.",
                    session.id
                ))
            }
            "close_session" => {
                let closed = other()?;
                self.peer_close(from, closed)?;
                Ok(format!(
                    "Closed session {closed}. The user can reopen it from the sidebar."
                ))
            }
            "set_mark" => self.peer_mark(from, arguments),
            "configure" => self.peer_configure(from, arguments),
            "use_skill" => {
                let session = self.session(from)?;
                let worktree = self.task_of(&session)?.worktree_path;
                self.skill_for_session(&session.agent, &worktree, word("name").unwrap_or_default())
            }
            _ => Err(refused(format!("Swarmed has no tool called `{tool}`"))),
        }
    }
}

/// Answers one connection: a request on one line, the answer on one line.
async fn answer(core: Arc<Core>, stream: UnixStream) -> std::io::Result<()> {
    let (reading, mut writing) = stream.into_split();
    let mut line = String::new();
    BufReader::new(reading).read_line(&mut line).await?;
    let request: Value = serde_json::from_str(&line).unwrap_or_default();
    let from = request.get("session").and_then(Value::as_i64).unwrap_or(0);
    let tool = request
        .get("tool")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let arguments = request.get("arguments").unwrap_or(&Value::Null);
    let called = match tool {
        DELEGATE_TO_CLOUD => core.peer_delegate(from, arguments).await,
        SHOW_BROWSER => core.browser_show_for(from, arguments).await,
        START_TASK => core.peer_start_task_said(from, arguments).await,
        _ => core.peer_call(from, tool, arguments),
    };
    let reply = match called {
        Ok(text) => json!({ "ok": true, "text": text }),
        Err(error) => json!({ "ok": false, "text": error.to_string() }),
    };
    writing.write_all(format!("{reply}\n").as_bytes()).await
}

/// Removes the sockets earlier runs left in `folder`: those of other processes that nobody
/// answers on any more.
async fn sweep(folder: &Path) {
    let Ok(mut entries) = tokio::fs::read_dir(folder).await else {
        return;
    };
    // A socket of this process is a core's that lives here, which may be between binding it
    // and listening on it, when it refuses a connection: only its owner replaces it. Tests run
    // many cores in one process at once, and one swept another's socket away.
    let this_process = format!("{SOCKET_PREFIX}{}-", std::process::id());
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        // Only Swarmed's own: the folder is shared with every other program's sockets.
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let ours = name.starts_with(SOCKET_PREFIX) && name.ends_with(".sock");
        let elsewhere = !name.starts_with(&this_process);
        if ours && elsewhere && UnixStream::connect(&path).await.is_err() {
            tokio::fs::remove_file(path).await.ok();
        }
    }
}

/// Listens for the tool servers of this core's sessions until the core's front end exits.
///
/// A front end runs this next to its sessions. Without it the session tools answer that they
/// are not available; showing views does not depend on it. From when it runs, each session
/// that starts is also given an address of the task's browser, which Swarmed listens at
/// (`browser/address.rs`).
pub async fn serve_session_tools(core: Arc<Core>) -> Result<()> {
    core.browsers_listen();
    let socket = core.paths().tools_socket();
    if let Some(folder) = socket.parent() {
        tokio::fs::create_dir_all(folder).await?;
        sweep(folder).await;
    }
    tokio::fs::remove_file(&socket).await.ok();
    let listener = UnixListener::bind(&socket)?;
    loop {
        let (stream, _) = listener.accept().await?;
        // Each request is answered on its own: a slow one must not hold up the others.
        drop(tokio::spawn(answer(Arc::clone(&core), stream)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_end_of_a_text_is_kept_whole_characters_and_all() {
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(tail("žąsis", 2), "is");
        assert_eq!(tail("ab", 9), "ab");
        assert_eq!(tail("", 3), "");
    }
}

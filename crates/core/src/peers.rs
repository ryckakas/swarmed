//! What sessions can do about each other: see who else is at work, read what another has said,
//! leave it a message, and start a new session beside themselves.
//!
//! Agents reach this through Swarmed's own tool server. That server is a process the agent
//! starts, so it asks the core over a socket in the data directory: one request on a line, one
//! answer on a line. Everything here is also plain methods on [`Core`], which is what the
//! tests use.

use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::agent::{AgentEvent, Auth};
use crate::cloud::{CloudOrigin, CloudProvider};
use crate::error::{Error, Result};
use crate::paths::SOCKET_PREFIX;
use crate::sessions::{Session, SessionOptions, SessionStatus};
use crate::toolserver::DELEGATE_TO_CLOUD;
use crate::{Core, Task};

/// How many sessions one session may have started that are still open. A runaway agent stops
/// here instead of filling the machine.
pub const MAX_SPAWNED: usize = 6;
/// How much of another session's conversation is handed over at most, in characters.
const DIGEST_CHARS: usize = 8000;
/// How many of a session's latest messages are read when no number is asked for.
const DIGEST_MESSAGES: usize = 12;

/// What agents are told about these tools, as part of Swarmed's guidance.
pub(crate) const INSTRUCTIONS: &str = "\
# Other sessions

You are one of possibly several agent sessions the user runs side by side in Swarmed, each in \
a task with its own git worktree. The `swarmed` tool server (in Claude Code: `mcp__swarmed__…`) \
lets you work with the others:
- `list_sessions`: who else is there, in which task, and whether they are busy.
- `read_session`: the latest messages of another session.
- `send_message`: leave another session a message. It arrives as a message in its \
conversation, at once if it is idle, else when its turn ends.
- `spawn_session`: start a new session in your own worktree with a prompt of yours, for a part \
of the work that can go on beside you. It shares your files, so give it a part that does not \
collide with what you are editing. It is told to report back to you with `send_message` when \
it is done; you do not have to wait for it.
- `close_session`: put away a session you spawned, once it is done and you have what you need \
from it. You cannot close any other session.
- `delegate_to_cloud`: hand a self-contained piece of work to an agent in the cloud, when the \
user wants something delegated or run there. It works on what is pushed to GitHub, not on \
your worktree, and does not report back to you.
- `configure`: change Swarmed's own settings when the user asks you to: keep a prompt, add a \
button, add a rule, change the status line or the colours. Some changes are only proposed, \
and made when the user accepts them.
- `set_mark`: put a colour and a few words on your own session in the sidebar, when the user, \
a skill or a prompt asks for one, such as green once a check has passed. It stays until the \
user writes to you again.

Use these when they help the user's task, not as a matter of course. A session you spawn costs \
the user money like any other.";

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

fn is_busy(session: &Session) -> bool {
    matches!(
        session.status,
        SessionStatus::Working | SessionStatus::Waiting
    )
}

/// One line about a session, as another session is told of it.
fn brief(session: &Session, task: &Task, own: i64) -> String {
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
    format!("{}{you}\t{state}\ttask {}\t{title}", session.id, task.name)
}

/// What a session said or was told, if the event is that, as one paragraph.
fn spoken(event: &AgentEvent) -> Option<String> {
    match event {
        AgentEvent::UserMessage { text } => Some(format!("User: {text}")),
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

impl Core {
    /// The task a session runs in.
    fn task_of(&self, session: &Session) -> Result<Task> {
        let task = self.store().task_by_id(session.task_id)?;
        task.ok_or(Error::SessionNotFound(session.id))
    }

    /// The open sessions of the project `from` works in, one line each, for an agent to read.
    pub fn peer_sessions(&self, from: i64) -> Result<String> {
        let own = self.session(from)?;
        let project = self.task_of(&own)?.project_id;
        let (tasks, sessions) = {
            let store = self.store();
            (store.tasks_all()?, store.sessions_all()?)
        };
        let in_project = |task: &&Task| task.project_id == project && !task.archived;
        let line = |session: &Session| {
            let task = tasks.iter().find(|task| task.id == session.task_id);
            task.filter(in_project)
                .map(|task| brief(session, task, from))
        };
        let mut lines = vec!["id\tstate\ttask\ttitle".to_string()];
        lines.extend(sessions.iter().filter_map(line));
        Ok(lines.join("\n"))
    }

    /// The latest `messages` things said in session `id`, by the user and by its agent, oldest
    /// first. Tool calls and their output are left out: they are the session's workings, and
    /// would drown what it concluded.
    pub fn peer_digest(&self, id: i64, messages: Option<usize>) -> Result<String> {
        let events = self.session_events(id)?;
        let said: Vec<String> = events.iter().filter_map(spoken).collect();
        let wanted = messages.unwrap_or(DIGEST_MESSAGES).max(1);
        let latest = &said[said.len().saturating_sub(wanted)..];
        if latest.is_empty() {
            return Ok("Nothing has been said in that session yet.".to_string());
        }
        Ok(tail(&latest.join("\n\n"), DIGEST_CHARS).to_string())
    }

    /// Leaves session `to` a message from session `from`. It arrives as a message in the
    /// conversation, marked with who sent it: now if `to` is idle, starting its agent if need
    /// be, else when its turn ends.
    pub fn peer_message(&self, from: i64, to: i64, text: &str) -> Result<Delivery> {
        let sender = self.session(from)?;
        let receiver = self.session(to)?;
        if from == to {
            return Err(refused("a session cannot send a message to itself"));
        }
        if text.trim().is_empty() {
            return Err(Error::EmptyMessage);
        }
        let title = if sender.title.is_empty() {
            "untitled"
        } else {
            &sender.title
        };
        let message = format!(
            "Message from session {from} (\"{title}\"), another agent session of the same \
             user:\n\n{text}"
        );
        if is_busy(&receiver) {
            self.session_enqueue(to, &message)?;
            return Ok(Delivery::Queued);
        }
        self.session_start(to, Auth::Subscription)?;
        self.session_send(to, &message)?;
        Ok(Delivery::Sent)
    }

    /// Starts a new session beside `from`, in the same task and so the same worktree, on the
    /// same agent with the same settings, and gives it `prompt`. The new session is told who
    /// started it and to report back when it is done.
    ///
    /// A session that was itself started this way cannot start others, and one session can
    /// have [`MAX_SPAWNED`] of them open at most.
    pub fn peer_spawn(&self, from: i64, prompt: &str, title: Option<&str>) -> Result<Session> {
        let parent = self.session(from)?;
        if prompt.trim().is_empty() {
            return Err(Error::EmptyMessage);
        }
        if parent.spawned_by.is_some() {
            return Err(refused(
                "a session that was started by another session cannot start sessions itself",
            ));
        }
        let open = self.store().sessions_spawned_by(from)?;
        if open >= MAX_SPAWNED {
            return Err(refused(format!(
                "this session already has {open} sessions of its own open, which is the most \
                 allowed; wait for them, or ask the user to close some"
            )));
        }
        let options = SessionOptions {
            agent: Some(parent.agent.clone()),
            title: title
                .map(String::from)
                .filter(|title| !title.trim().is_empty()),
            model: parent.model.clone(),
            supervision: parent.supervision,
            effort: parent.effort,
            fast: parent.fast,
        };
        let child = {
            let store = self.store();
            let child = store.session_insert(parent.task_id, &options)?;
            store.session_set_spawned_by(child.id, from)?;
            child
        };
        let message = format!(
            "{prompt}\n\n(You were started by session {from}, another agent session of the same \
             user, working in this same worktree. When you are done, report what you did and \
             found to it with the `send_message` tool, to session {from}.)"
        );
        self.session_start(child.id, Auth::Subscription)?;
        self.session_send(child.id, &message)?;
        self.session(child.id)
    }

    /// Closes session `id` on behalf of session `from`, which has to be the one that started
    /// it, and only once it is idle. A session the user started, or another agent's, is not an
    /// agent's to put away; and closing one in the middle of a turn would throw its work away.
    pub fn peer_close(&self, from: i64, id: i64) -> Result<()> {
        self.session(from)?;
        let session = self.session(id)?;
        if session.spawned_by != Some(from) {
            return Err(refused(
                "a session can only close sessions it started itself; this one is not yours",
            ));
        }
        if is_busy(&session) {
            return Err(refused(
                "that session is still at work; wait until it is done before closing it",
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
                let messages = number("messages").and_then(|n| usize::try_from(n).ok());
                self.peer_digest(other()?, messages)
            }
            "send_message" => {
                let text = word("text").unwrap_or_default();
                let delivery = self.peer_message(from, other()?, text)?;
                Ok(delivery.said().to_string())
            }
            "spawn_session" => {
                let session =
                    self.peer_spawn(from, word("prompt").unwrap_or_default(), word("title"))?;
                Ok(format!(
                    "Started session {}. It works beside you in this worktree and will report \
                     back with a message when it is done.",
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
    let called = if tool == DELEGATE_TO_CLOUD {
        core.peer_delegate(from, arguments).await
    } else {
        core.peer_call(from, tool, arguments)
    };
    let reply = match called {
        Ok(text) => json!({ "ok": true, "text": text }),
        Err(error) => json!({ "ok": false, "text": error.to_string() }),
    };
    writing.write_all(format!("{reply}\n").as_bytes()).await
}

/// Removes the sockets earlier runs left in `folder`: those nobody answers on any more.
async fn sweep(folder: &Path) {
    let Ok(mut entries) = tokio::fs::read_dir(folder).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        // Only Swarmed's own: the folder is shared with every other program's sockets.
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let ours = name.starts_with(SOCKET_PREFIX) && name.ends_with(".sock");
        if ours && UnixStream::connect(&path).await.is_err() {
            tokio::fs::remove_file(path).await.ok();
        }
    }
}

/// Listens for the tool servers of this core's sessions until the core's front end exits.
///
/// A front end runs this next to its sessions. Without it the session tools answer that they
/// are not available; showing views does not depend on it.
pub async fn serve_session_tools(core: Arc<Core>) -> Result<()> {
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

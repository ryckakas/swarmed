//! Taking a session back to before one of its messages, its files too if the user asks (#364).
//!
//! What follows the message stays stored: costs and search still count it, and front ends show
//! it folded. An [`AgentEvent::Rewound`] says it no longer counts. Claude Code resumes its own
//! record of the conversation at the last message before that one; any other agent, and Claude
//! Code where no such point was recorded, starts a new conversation and is told what was said
//! up to there with the next message, as a fork's agent is.
//!
//! The files go back to the checkpoint taken before the message (`checkpoints.rs`), after the
//! worktree as it is now has been kept, so that they can be put back. Nothing is tried that
//! cannot be done safely: while any agent could write to the worktree, or a rebase or a merge
//! is under way in it, it is refused. Commits stay on the branch. Claude Code's own
//! `rewind_files` is never used: the files go back the same way whatever the agent.

use std::collections::HashSet;
use std::ops::Range;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use crate::agent::AgentEvent;
use crate::changes::FileChange;
use crate::checkpoints::{self, Left, Stopped};
use crate::error::{Error, Result};
use crate::sessions::{Live, Session, is_busy_status};
use crate::store::Store;
use crate::store::Task;
use crate::{CLAUDE, Core, lock};

/// The places in `events` of the parts that rewinds dropped: from the message each took the
/// session back to before, up to the rewind itself.
pub(crate) fn dropped(events: &[AgentEvent]) -> Vec<Range<usize>> {
    let messages: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, event)| matches!(event, AgentEvent::UserMessage { .. }))
        .map(|(at, _)| at)
        .collect();
    let range = |(at, event): (usize, &AgentEvent)| match event {
        AgentEvent::Rewound { to, .. } => {
            let start = messages.get(*to).copied().filter(|start| *start < at)?;
            Some(start..at)
        }
        _ => None,
    };
    events.iter().enumerate().filter_map(range).collect()
}

/// `events` without what rewinds dropped: the conversation as it counts now.
pub(crate) fn counted(events: &[AgentEvent]) -> impl Iterator<Item = &AgentEvent> {
    let dropped = dropped(events);
    let counts = move |at: &usize| !dropped.iter().any(|range| range.contains(at));
    events
        .iter()
        .enumerate()
        .filter(move |(at, _)| counts(at))
        .map(|(_, event)| event)
}

/// Where message number `to` stands in `events`, counting from 0 every message sent into the
/// session, if it is still in the conversation: not one a rewind dropped.
fn message_at(events: &[AgentEvent], to: usize) -> Option<usize> {
    let at = events
        .iter()
        .enumerate()
        .filter(|(_, event)| matches!(event, AgentEvent::UserMessage { .. }))
        .nth(to)
        .map(|(at, _)| at)?;
    let dropped = dropped(events);
    (!dropped.iter().any(|range| range.contains(&at))).then_some(at)
}

/// Where Claude Code resumes a conversation taken back to before the message at place `at` of
/// `events`: its id of the last message of the turn before, if that turn, begun by the message
/// before, ended with one in `conversation`, the agent's conversation now. `None` when there is
/// no message before, or nothing was kept of that turn, such as one that failed, one from
/// before such points were recorded, or one in a conversation the agent has since left: it
/// then starts a new conversation.
fn resume_point(events: &[AgentEvent], at: usize, conversation: Option<&str>) -> Option<String> {
    let conversation = conversation?;
    let before = &events[..at];
    let dropped = dropped(before);
    let counts = |at: &usize| !dropped.iter().any(|range| range.contains(at));
    let last_message = before
        .iter()
        .enumerate()
        .filter(|(at, _)| counts(at))
        .rfind(|(_, event)| matches!(event, AgentEvent::UserMessage { .. }))?
        .0;
    // Which conversation each point is in: the one the agent last said it opened.
    let mut opened = None;
    let mut point = None;
    for (place, event) in before.iter().enumerate() {
        match event {
            AgentEvent::Init {
                agent_session_id, ..
            } => opened = Some(agent_session_id.as_str()),
            AgentEvent::ResumePoint { uuid } if place > last_message && counts(&place) => {
                point = (opened == Some(conversation)).then(|| uuid.clone());
            }
            _ => {}
        }
    }
    point
}

/// Where Claude Code is to resume the conversation of a session whose events are `events`: at
/// the point the latest rewind took it back to, until a turn after it has ended with a point of
/// its own. From then on Claude Code's latest messages are those after the rewind, which a
/// plain resume carries on from.
pub(crate) fn resume_at(events: &[AgentEvent]) -> Option<String> {
    let at = events
        .iter()
        .rposition(|event| matches!(event, AgentEvent::Rewound { .. }))?;
    let AgentEvent::Rewound { resume_at, .. } = &events[at] else {
        return None;
    };
    let moved_on = events[at..]
        .iter()
        .any(|event| matches!(event, AgentEvent::ResumePoint { .. }));
    if moved_on { None } else { resume_at.clone() }
}

/// Whether an agent is to be told what was said before `event` with its next message: after a
/// fork, and after a rewind in which it started a new conversation.
pub(crate) fn starts_anew(event: &AgentEvent) -> bool {
    matches!(
        event,
        AgentEvent::Forked { .. }
            | AgentEvent::Rewound {
                resume_at: None,
                ..
            }
    )
}

/// What taking a session back to before one of its messages would do now, for the choice that
/// offers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RewindCheck {
    /// Why the session cannot be taken back to before the message now, if it cannot.
    pub refused: Option<String>,
    /// Why its files cannot be taken back with it now, if they cannot. The conversation alone
    /// still can, unless `refused` says otherwise.
    pub files_refused: Option<String>,
    /// Whether the agent resumes its own record of the conversation at the message. Otherwise
    /// it starts a new conversation and is told what was said up to there.
    pub resumes: bool,
    /// What taking the files back would change, from how they are now to how they were before
    /// the message. Empty when they cannot be taken back, or nothing differs.
    pub files: Vec<FileChange>,
    /// How many commits were made on the worktree's branch since the message was sent: they
    /// stay there. `None` when that is not known.
    pub commits_since: Option<usize>,
}

/// What came of taking a session back to before one of its messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RewindDone {
    /// The message it was taken back to before.
    pub to: usize,
    /// Whether the files were taken back too.
    pub files: bool,
    /// Whether the agent resumes its own record of the conversation there.
    pub resumes: bool,
    /// How many commits made since the message stay on the branch, if that is known.
    pub commits_since: Option<usize>,
}

/// Where a session goes back to: the message, its place among the events, and where its
/// agent resumes, if it does.
struct Plan {
    session: Session,
    task: Task,
    to: usize,
    resume_at: Option<String>,
}

/// While a session of a task is taken back: no message goes to an agent of that task, since
/// one could write into the worktree as its files go back, or start the agent that is being
/// taken back.
pub(crate) struct TakingBack<'a> {
    live: &'a Live,
    task: i64,
}

impl Drop for TakingBack<'_> {
    fn drop(&mut self) {
        lock(&self.live.taking_back).remove(&self.task);
    }
}

impl Live {
    /// Refuses a message to session `id` while a session of its task is being taken back.
    /// Otherwise no rewind begins until what is returned is dropped: held until the message is
    /// recorded, the rewind that comes next finds the session at work, where one that looked
    /// in between the check and the record would have found it idle and taken its files back
    /// under the turn.
    pub(crate) fn not_taking_back(
        &self,
        store: &Mutex<Store>,
        id: i64,
    ) -> Result<MutexGuard<'_, HashSet<i64>>> {
        let taking = lock(&self.taking_back);
        if taking.is_empty() {
            return Ok(taking);
        }
        let task = lock(store).session_by_id(id).map(|session| session.task_id);
        if task.is_ok_and(|task| taking.contains(&task)) {
            return Err(taking_back_refusal());
        }
        Ok(taking)
    }
}

/// Why nothing is sent while a session of the task is taken back.
pub(crate) fn taking_back_refusal() -> Error {
    Error::Rewind(
        "a session of this task is being taken back to an earlier message: send it again in a \
         moment"
            .to_string(),
    )
}

impl Core {
    /// What taking session `id` back to before its message number `to` would do now: whether it
    /// can be, with its files or without, and what the files and the branch would look like.
    /// Changes nothing.
    pub async fn session_rewind_check(&self, id: i64, to: usize) -> Result<RewindCheck> {
        let plan = match self.rewind_plan(id, to) {
            Ok(plan) => plan,
            Err(error) => return Ok(refused_whole(&error)),
        };
        let resumes = plan.resume_at.is_some();
        let before = match self.files_may_go_back(&plan) {
            Ok(before) => before,
            Err(error) => {
                return Ok(RewindCheck {
                    refused: None,
                    files_refused: Some(error.to_string()),
                    resumes,
                    files: vec![],
                    commits_since: None,
                });
            }
        };
        let (worktree, store) = (
            plan.task.worktree_path.clone(),
            self.paths.checkpoints(plan.task.id),
        );
        let (tree, head) = before;
        let looked = tokio::task::spawn_blocking(move || {
            let files = checkpoints::changed_since(&worktree, &store, &tree);
            let commits = head.and_then(|head| checkpoints::commits_since(&worktree, &head));
            (files, commits)
        })
        .await
        .map_err(|error| Error::Io(std::io::Error::other(error)))?;
        let (files, commits_since) = looked;
        let (files, files_refused) = match files {
            Ok(files) => (files, None),
            Err(error) => (vec![], Some(error.to_string())),
        };
        Ok(RewindCheck {
            refused: None,
            files_refused,
            resumes,
            files,
            commits_since,
        })
    }

    /// Takes session `id` back to before its message number `to`, counting from 0 every message
    /// sent into it: that message and what followed no longer count, and its agent goes on from
    /// there with the next message, which the user writes. With `files`, the worktree's files
    /// go back to the checkpoint taken before the message too, once the worktree as it is now
    /// has been kept, so that [`Core::session_files_put_back`] can put them back.
    ///
    /// Refused, with the reason, while the session is at work, closed, or run by another
    /// Swarmed, or its agent left something running; and with `files`, also while any session
    /// of the task is at work or run elsewhere, anything its agents left runs, its setup runs,
    /// a rebase or a merge is under way, or there is no checkpoint before that message.
    pub async fn session_rewind(&self, id: i64, to: usize, files: bool) -> Result<RewindDone> {
        let plan = self.rewind_plan(id, to)?;
        let _held = self.take_back(plan.task.id)?;
        // Asked again under the hold, which no new turn gets past.
        let plan = self.rewind_plan(id, to)?;
        let before = files.then(|| self.files_may_go_back(&plan)).transpose()?;
        // Its agent remembers the conversation: it is stopped, and goes on from the point
        // chosen when it starts again.
        self.session_ended(id).await;
        let commits_since = match before {
            Some(before) => self.files_back(&plan, before).await?,
            None => None,
        };
        let resumes = plan.resume_at.is_some();
        if !resumes {
            self.store().session_forget_conversation(id)?;
        }
        let event = AgentEvent::Rewound {
            to,
            files,
            resume_at: plan.resume_at,
            commits_since,
        };
        self.live.note(&self.store, id, event)?;
        Ok(RewindDone {
            to,
            files,
            resumes,
            commits_since,
        })
    }

    /// Puts session `id`'s files back as they were before its latest rewind took them back.
    /// Refused if nothing it took back can be put back, if the files changed since, and for
    /// what refuses taking files back at all: an agent of the task at work or run elsewhere,
    /// something left running, the setup, a rebase or a merge.
    pub async fn session_files_put_back(&self, id: i64) -> Result<()> {
        let session = self.session(id)?;
        let task = self.task_of(&session)?;
        let _held = self.take_back(task.id)?;
        let kept = self.store().rewind_latest(id)?.ok_or_else(|| {
            Error::Rewind("no files this session took back can be put back".to_string())
        })?;
        self.task_quiet(&task)?;
        let (worktree, store) = (task.worktree_path.clone(), self.paths.checkpoints(task.id));
        let (was, restored) = (kept.was.clone(), kept.restored.clone());
        let put_back = tokio::task::spawn_blocking(move || {
            checkpoints::restore(&worktree, &store, &was, Some(&restored))
        })
        .await
        .map_err(|error| Error::Io(std::io::Error::other(error)))?;
        if let Err(stopped) = put_back {
            // Left part-way: what was kept is still what to put back, from where they are now.
            let kept = (kept.message, kept.was.as_str());
            return Err(self.stopped(id, kept, stopped));
        }
        self.store().rewind_forget(id)?;
        let event = AgentEvent::FilesPutBack { to: kept.message };
        self.live.note(&self.store, id, event)
    }

    /// Where session `id` goes back to for its message number `to`, if it can now.
    fn rewind_plan(&self, id: i64, to: usize) -> Result<Plan> {
        let session = self.session(id)?;
        if self.store().session_is_closed(id)? {
            return Err(Error::SessionClosed(id));
        }
        if self.runs_elsewhere(id) {
            return Err(Error::RunsElsewhere(id));
        }
        if is_busy_status(session.status) {
            return Err(Error::Rewind(
                "the session is at work: stop its turn first".to_string(),
            ));
        }
        let task = self.task_of(&session)?;
        if self.background_running_in(task.id, Some(id))? {
            return Err(Error::Rewind(
                "its agent left something running, which would end with it: stop that first, \
                 from the task's Background list"
                    .to_string(),
            ));
        }
        let events = self.session_events(id)?;
        let at = message_at(&events, to).ok_or_else(|| {
            Error::Rewind(format!(
                "message {to} is not in the conversation: it was never sent, or a rewind \
                 dropped it already"
            ))
        })?;
        let conversation = session.agent_session_id.as_deref();
        let resume_at = (session.agent == CLAUDE)
            .then(|| resume_point(&events, at, conversation))
            .flatten();
        Ok(Plan {
            session,
            task,
            to,
            resume_at,
        })
    }

    /// The checkpoint the files of `plan` go back to, its tree and the commit it was on, if
    /// they can go back now.
    fn files_may_go_back(&self, plan: &Plan) -> Result<(String, Option<String>)> {
        if self.is_chat(&plan.task) {
            return Err(Error::NoRepository);
        }
        let before = self.checkpoint_before(plan.session.id, plan.to)?;
        let before = before.ok_or(Error::TurnNotKept {
            session: plan.session.id,
            message: plan.to,
        })?;
        self.task_quiet(&plan.task)?;
        Ok(before)
    }

    /// Refuses, with the reason, while anything could write into `task`'s worktree: a session
    /// of it at work or run by another Swarmed, something its agents left running, its setup.
    fn task_quiet(&self, task: &Task) -> Result<()> {
        let sessions = self.store().sessions(task.id)?;
        if let Some(busy) = sessions.iter().find(|s| is_busy_status(s.status)) {
            return Err(Error::Rewind(format!(
                "{} is at work in this worktree: wait for it, or stop it first",
                named(busy)
            )));
        }
        if let Some(elsewhere) = sessions.iter().find(|s| self.runs_elsewhere(s.id)) {
            return Err(Error::Rewind(format!(
                "{} runs in another Swarmed, which could write into this worktree",
                named(elsewhere)
            )));
        }
        if self.background_running_in(task.id, None)? {
            return Err(Error::Rewind(
                "something an agent left running in this worktree still runs: stop it first, \
                 from the task's Background list"
                    .to_string(),
            ));
        }
        if self.setup_runs(task) {
            return Err(Error::Rewind(
                "the task's setup is running in this worktree".to_string(),
            ));
        }
        Ok(())
    }

    /// Takes the files of `plan` back to the checkpoint `before`, after keeping the worktree as
    /// it is, and says how many commits made since stay on the branch.
    async fn files_back(
        &self,
        plan: &Plan,
        (tree, head): (String, Option<String>),
    ) -> Result<Option<usize>> {
        let worktree = plan.task.worktree_path.clone();
        let store = self.paths.checkpoints(plan.task.id);
        let to = tree.clone();
        let at = worktree.clone();
        let restored =
            tokio::task::spawn_blocking(move || checkpoints::restore(&at, &store, &to, None))
                .await
                .map_err(|error| Error::Io(std::io::Error::other(error)))?;
        let session = plan.session.id;
        let was = match restored {
            Ok(was) => was,
            Err(stopped) => {
                let was = stopped.left.as_ref().map(|left| left.was.clone());
                let kept = (plan.to, was.as_deref().unwrap_or_default());
                return Err(self.stopped(session, kept, stopped));
            }
        };
        self.store()
            .rewind_keep(session, plan.to, &was.tree, &tree)?;
        let commits = tokio::task::spawn_blocking(move || {
            head.and_then(|head| checkpoints::commits_since(&worktree, &head))
        })
        .await
        .map_err(|error| Error::Io(std::io::Error::other(error)))?;
        Ok(commits)
    }

    /// Why the files of session `id` were not taken to another tree, after keeping what git
    /// left part-way: the worktree as it was for message `message`, tree `was`, can then still
    /// be put back from where the files are now.
    fn stopped(&self, id: i64, (message, was): (usize, &str), stopped: Stopped) -> Error {
        let Some(Left { now, .. }) = stopped.left else {
            return stopped.why;
        };
        if let Err(error) = self.store().rewind_keep(id, message, was, &now) {
            return error;
        }
        Error::Rewind(format!(
            "{}. Some files were changed by then and could not be put as they were: once what \
             stopped git is put right, `swarmed session put-back {id}` puts them back",
            stopped.why
        ))
    }

    /// Holds off every message to an agent of task `task` until the hold is dropped. Refused
    /// while another session of it is being taken back.
    fn take_back(&self, task: i64) -> Result<TakingBack<'_>> {
        if !lock(&self.live.taking_back).insert(task) {
            return Err(Error::Rewind(
                "another session of this task is being taken back: try again in a moment"
                    .to_string(),
            ));
        }
        Ok(TakingBack {
            live: &self.live,
            task,
        })
    }
}

/// A check for a rewind refused as a whole, with the reason.
fn refused_whole(error: &Error) -> RewindCheck {
    RewindCheck {
        refused: Some(error.to_string()),
        files_refused: None,
        resumes: false,
        files: vec![],
        commits_since: None,
    }
}

/// A session as a refusal names it: by its title, or its number if it has none.
fn named(session: &Session) -> String {
    if session.title.is_empty() {
        format!("session {}", session.id)
    } else {
        format!("“{}”", session.title)
    }
}

/// A rewind of a session's files that can still be put back.
struct Kept {
    message: usize,
    was: String,
    restored: String,
}

impl Store {
    /// Keeps the worktree as it was before session `id`'s files were taken back to `restored`
    /// for its message `message`, in place of any rewind of its files kept before: only the
    /// latest can be put back.
    fn rewind_keep(&self, id: i64, message: usize, was: &str, restored: &str) -> Result<()> {
        self.rewind_forget(id)?;
        self.conn.execute(
            "INSERT INTO rewinds (session_id, message, was, restored, at_ms) \
             VALUES (?1, ?2, ?3, ?4, CAST(strftime('%s', 'now') AS INTEGER) * 1000)",
            params![
                id,
                i64::try_from(message).unwrap_or(i64::MAX),
                was,
                restored
            ],
        )?;
        Ok(())
    }

    /// The latest rewind of session `id`'s files, if it can still be put back.
    fn rewind_latest(&self, id: i64) -> Result<Option<Kept>> {
        let kept = self
            .conn
            .query_row(
                "SELECT message, was, restored FROM rewinds WHERE session_id = ?1 \
                 ORDER BY id DESC LIMIT 1",
                params![id],
                |row| {
                    Ok(Kept {
                        message: usize::try_from(row.get::<_, i64>(0)?).unwrap_or(0),
                        was: row.get(1)?,
                        restored: row.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(kept)
    }

    /// Forgets what session `id`'s rewinds could put back.
    fn rewind_forget(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM rewinds WHERE session_id = ?1", params![id])?;
        Ok(())
    }

    /// Forgets the agent's own conversation of session `id`: its agent starts a new one.
    fn session_forget_conversation(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET agent_session_id = NULL WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> AgentEvent {
        AgentEvent::UserMessage {
            text: text.to_string(),
            remote: false,
            at_reset: false,
        }
    }

    fn reply(text: &str) -> AgentEvent {
        AgentEvent::Text {
            text: text.to_string(),
        }
    }

    fn point(uuid: &str) -> AgentEvent {
        AgentEvent::ResumePoint {
            uuid: uuid.to_string(),
        }
    }

    fn rewound(to: usize, resume_at: Option<&str>) -> AgentEvent {
        AgentEvent::Rewound {
            to,
            files: false,
            resume_at: resume_at.map(String::from),
            commits_since: None,
        }
    }

    fn texts<'a>(events: impl Iterator<Item = &'a AgentEvent>) -> Vec<String> {
        events
            .filter_map(|event| match event {
                AgentEvent::UserMessage { text, .. } | AgentEvent::Text { text } => {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn what_a_rewind_dropped_no_longer_counts_and_one_inside_another_is_dropped_whole() {
        let events = [
            user("one"),
            reply("1"),
            user("two"),
            reply("2"),
            rewound(1, None),
            user("three"),
            reply("3"),
            user("four"),
            // Messages are counted whole, those dropped before too: "four" is the fourth.
            rewound(3, None),
            user("five"),
        ];
        assert_eq!(dropped(&events), [2..4, 7..8]);
        assert_eq!(texts(counted(&events)), ["one", "1", "three", "3", "five"]);
        // A rewind to before the first message drops all that came before it.
        let all = [user("one"), reply("1"), rewound(0, None), user("again")];
        assert_eq!(texts(counted(&all)), ["again"]);
        let outer = [
            user("one"),
            user("two"),
            rewound(1, None),
            user("three"),
            rewound(0, None),
        ];
        assert_eq!(dropped(&outer), [1..2, 0..4]);
        assert_eq!(counted(&outer).count(), 1);
    }

    #[test]
    fn only_a_message_still_in_the_conversation_can_be_gone_back_to() {
        let events = [user("one"), user("two"), rewound(1, None), user("three")];
        assert_eq!(message_at(&events, 0), Some(0));
        // Dropped by the rewind, and never sent.
        assert_eq!(message_at(&events, 1), None);
        assert_eq!(message_at(&events, 5), None);
        assert_eq!(message_at(&events, 2), Some(3));
    }

    fn opened(conversation: &str) -> AgentEvent {
        AgentEvent::Init {
            agent_session_id: conversation.to_string(),
            model: String::new(),
            permission_mode: String::new(),
            api_key_source: String::new(),
            mcp_servers: vec![],
            skills: 0,
            slash_commands: vec![],
            capabilities: vec![],
        }
    }

    #[test]
    fn claude_code_resumes_where_the_turn_before_the_message_ended() {
        let events = [
            user("one"),
            opened("a"),
            reply("1"),
            point("p1"),
            user("two"),
            reply("2"),
            point("p2"),
            user("three"),
        ];
        let at = |events: &[AgentEvent], place| resume_point(events, place, Some("a"));
        assert_eq!(at(&events, 7).as_deref(), Some("p2"));
        assert_eq!(at(&events, 4).as_deref(), Some("p1"));
        // Nothing before the first message: a new conversation.
        assert_eq!(at(&events, 0), None);
        // An agent that has no conversation now has none to resume.
        assert_eq!(resume_point(&events, 7, None), None);
        // A turn that kept nothing: resuming at the one before would leave its message out.
        let failed = [
            user("one"),
            opened("a"),
            point("p1"),
            user("two"),
            user("three"),
        ];
        assert_eq!(at(&failed, 4), None);
        // A part an earlier rewind dropped is passed over: the agent went on from before it.
        let again = [
            user("one"),
            opened("a"),
            point("p1"),
            user("two"),
            point("p2"),
            rewound(1, Some("p1")),
            user("three"),
            point("p3"),
            user("four"),
        ];
        assert_eq!(at(&again, 8).as_deref(), Some("p3"));
        assert_eq!(at(&again, 6).as_deref(), Some("p1"));
    }

    #[test]
    fn a_point_of_a_conversation_the_agent_has_left_is_not_resumed_at() {
        // Conversation `a`, then a rewind that started `b`.
        let events = [
            user("one"),
            opened("a"),
            point("a1"),
            user("two"),
            rewound(1, None),
            user("two again"),
            opened("b"),
            point("b1"),
            user("three"),
        ];
        assert_eq!(resume_point(&events, 5, Some("b")), None);
        assert_eq!(resume_point(&events, 8, Some("b")).as_deref(), Some("b1"));
    }

    #[test]
    fn the_point_a_rewind_chose_holds_until_a_turn_after_it_ends_with_its_own() {
        let mut events = vec![user("one"), point("p1"), user("two"), point("p2")];
        assert_eq!(resume_at(&events), None);
        events.push(rewound(1, Some("p1")));
        assert_eq!(resume_at(&events).as_deref(), Some("p1"));
        // Started again, and the turn failed before a word: still from there.
        events.push(user("two again"));
        assert_eq!(resume_at(&events).as_deref(), Some("p1"));
        events.push(point("p3"));
        assert_eq!(resume_at(&events), None);
        assert_eq!(resume_at(&[user("one"), rewound(0, None)]), None);
    }

    #[test]
    fn a_new_conversation_is_told_what_was_said_and_a_resumed_one_is_not() {
        assert!(starts_anew(&AgentEvent::Forked { from: 1 }));
        assert!(starts_anew(&rewound(1, None)));
        assert!(!starts_anew(&rewound(1, Some("p1"))));
    }
}

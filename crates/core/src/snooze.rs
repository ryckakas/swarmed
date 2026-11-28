//! A snooze: the user puts a session out of sight until a time (#377).
//!
//! Many sessions are idle and done for now but not finished: waiting on a review, a colleague,
//! tomorrow. Reading one loses the reminder, and closing it stops its agent. A snooze says "not
//! now, show me this again at 9:00".
//!
//! - **The user's attention, never the agent.** A snoozed session keeps running whatever it
//!   runs: nothing is stopped, sent or queued. Front ends leave it out of their lists, the inbox
//!   and the counts of what waits for the user until its time ([`Session::snoozed_until`]), and
//!   still show it moving while it works: running work is never hidden.
//! - **Back on time with nothing acting.** Only the time is kept, in `sessions.snoozed_until`,
//!   and the time passing changes nothing stored: a time that has passed is as good as none,
//!   and each front end draws again when it comes. So no follower, timer or house is needed,
//!   and a Swarmed that was not running at the time shows the session as soon as it starts.
//! - **Back early when it needs the user.** Recording an event clears it ([`wakes`], from
//!   `Store::record`) for a message the user writes, in Swarmed or on claude.ai, a permission
//!   request (a question is one) and an exit with an error, as a message clears a mark. A
//!   message another session left (`from_session`, `peers.rs`) does not, nor the one Swarmed
//!   sends by itself when a usage limit resets (`at_reset`): neither is the user's, and a
//!   planner snoozed until Monday must not come back each time a worker reports. A turn that
//!   ends well does not wake it either: the session is unread when its time comes, and its
//!   "Finished" notice is not given meanwhile (`notices.rs`). Closing clears it too; a fork
//!   starts without one.
//! - **Not an event.** As a highlight is (`highlights.rs`), a snooze is in the tree, not in the
//!   conversation: the change is published as [`AgentEvent::TreeChanged`] on session 0, and
//!   another Swarmed sees the row change through the tree's fingerprint (`elsewhere.rs`). A
//!   clear the record makes is told by the event that made it, which front ends follow.
//! - **Counted as the sidebar draws it.** A move among siblings passes over a snoozed session,
//!   and the sessions that work for one are drawn at their task's top level (`order.rs`). A
//!   snoozed session keeps its task from being put away by itself (`put_away.rs`): the user
//!   asked to be reminded of it.
//! - **Refused** for a closed session ([`Error::SessionClosed`]), which is out of sight
//!   already, and for a time that has come ([`Error::SnoozePast`]), which would wake it at once.
//!
//! The window offers fixed choices (in an hour, this evening, tomorrow morning, next Monday)
//! whose hours are the user's settings (`Settings::snooze_morning_hour`,
//! `Settings::snooze_evening_hour`), and the command line takes a time or a duration. Either
//! way the core is handed a time.

use rusqlite::params;

use crate::Core;
use crate::account_usage::seconds_now;
use crate::agent::AgentEvent;
use crate::error::{Error, Result};
use crate::sessions::{Session, SessionStatus};
use crate::store::Store;

/// Whether recording `event`, which leads to `status`, wakes its session: the user wrote to
/// it, here or on claude.ai, its agent asks for a decision, or its agent stopped with an error.
/// Each needs the user now, whatever time the snooze was for. A message another session left
/// does not, nor the one Swarmed sends by itself when a usage limit resets: the user did not
/// write either.
pub(crate) fn wakes(event: &AgentEvent, status: Option<SessionStatus>) -> bool {
    match event {
        AgentEvent::UserMessage {
            at_reset,
            from_session,
            ..
        } => !at_reset && from_session.is_none(),
        AgentEvent::PermissionRequest { .. } => true,
        _ => status == Some(SessionStatus::Errored),
    }
}

/// Whether `session` is snoozed now: until a time still to come.
pub(crate) fn snoozed_now(session: &Session) -> bool {
    session
        .snoozed_until
        .is_some_and(|until| until > seconds_now())
}

impl Core {
    /// Snoozes session `id` until `until`, in seconds since 1970, in place of any snooze it
    /// has, or with `None` wakes it, and has every front end read the tree again. Its agent is
    /// left as it is. Refused for a closed session ([`Error::SessionClosed`]) and for a time
    /// that is not later than now ([`Error::SnoozePast`]); waking one that is awake changes
    /// nothing.
    pub fn session_snooze(&self, id: i64, until: Option<i64>) -> Result<Session> {
        self.session(id)?;
        if self.store().session_is_closed(id)? {
            return Err(Error::SessionClosed(id));
        }
        if let Some(until) = until.filter(|until| *until <= seconds_now()) {
            return Err(Error::SnoozePast { id, until });
        }
        self.store().snooze_write(id, until)?;
        self.live.publish_apart(AgentEvent::TreeChanged);
        self.session(id)
    }
}

impl Store {
    /// Writes until when session `id` is snoozed, or with `None` that it is awake.
    pub(crate) fn snooze_write(&self, id: i64, until: Option<i64>) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET snoozed_until = ?2 WHERE id = ?1",
            params![id, until],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn what_needs_the_user_wakes_a_session_and_a_turn_that_ends_well_does_not() {
        let message = AgentEvent::UserMessage {
            text: "go on".to_string(),
            remote: false,
            at_reset: false,
            from_session: None,
        };
        let request = AgentEvent::PermissionRequest {
            request_id: "r1".to_string(),
            tool_name: "Bash".to_string(),
            input: Value::Null,
            description: None,
        };
        let failed = AgentEvent::Exited {
            code: Some(1),
            stderr: String::new(),
        };
        let stopped = AgentEvent::Exited {
            code: Some(0),
            stderr: String::new(),
        };
        let done = AgentEvent::TurnDone {
            is_error: false,
            result: Some("done".to_string()),
            cost_usd: None,
            duration_ms: None,
            tokens: None,
        };
        let status = |event: &AgentEvent| SessionStatus::after(event, false);
        assert!(wakes(&message, status(&message)));
        // What the user did not write leaves it asleep: another session's message, and the
        // one Swarmed sends by itself at a usage limit's reset.
        let left = AgentEvent::UserMessage {
            text: "Message from session 4 (\"Tests\"), another agent session of the same user:\n\ndone"
                .to_string(),
            remote: false,
            at_reset: false,
            from_session: Some(4),
        };
        assert!(!wakes(&left, status(&left)));
        let at_reset = AgentEvent::UserMessage {
            text: "Continue where you stopped.".to_string(),
            remote: false,
            at_reset: true,
            from_session: None,
        };
        assert!(!wakes(&at_reset, status(&at_reset)));
        assert!(wakes(&request, SessionStatus::after(&request, true)));
        assert!(wakes(&failed, status(&failed)));
        assert!(!wakes(&stopped, status(&stopped)));
        assert!(!wakes(&done, status(&done)));
    }

    #[test]
    fn recording_what_needs_the_user_clears_the_snooze_and_the_rest_keeps_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::open(&dir.path().join("swarmed.db")).expect("opens");
        let project = store.project_insert("p", dir.path()).expect("inserts");
        let task = store
            .task_insert(&crate::store::NewTask {
                project_id: project.id,
                name: "t",
                branch: "b",
                base_ref: "main",
                worktree_path: dir.path(),
                slot: 1,
                environment: None,
                branch_made: true,
                issue: None,
            })
            .expect("inserts");
        let options = crate::SessionOptions::default();
        let id = store.session_insert(task.id, &options).expect("inserts").id;
        let later = seconds_now() + 3600;
        let snoozed = || store.session_by_id(id).expect("there").snoozed_until;
        let record = |event: AgentEvent, status: Option<SessionStatus>| {
            store.snooze_write(id, Some(later)).expect("snoozes");
            store.record(id, &event, status, None).expect("records");
        };
        let said = AgentEvent::Text {
            text: "looking".to_string(),
        };
        record(said, Some(SessionStatus::Working));
        assert_eq!(snoozed(), Some(later), "work goes on asleep");
        let done = AgentEvent::TurnDone {
            is_error: false,
            result: None,
            cost_usd: None,
            duration_ms: None,
            tokens: None,
        };
        record(done, Some(SessionStatus::Idle));
        assert_eq!(snoozed(), Some(later), "a turn that ends well keeps it");
        let left = AgentEvent::UserMessage {
            text: "done with the tests".to_string(),
            remote: false,
            at_reset: false,
            from_session: Some(4),
        };
        record(left, Some(SessionStatus::Working));
        assert_eq!(snoozed(), Some(later), "a worker's report keeps it");
        let at_reset = AgentEvent::UserMessage {
            text: "Continue where you stopped.".to_string(),
            remote: false,
            at_reset: true,
            from_session: None,
        };
        record(at_reset, Some(SessionStatus::Working));
        assert_eq!(snoozed(), Some(later), "going on at a reset keeps it");
        let request = AgentEvent::PermissionRequest {
            request_id: "r1".to_string(),
            tool_name: "AskUserQuestion".to_string(),
            input: Value::Null,
            description: None,
        };
        record(request, Some(SessionStatus::Waiting));
        assert_eq!(snoozed(), None, "a question wakes it");
        let failed = AgentEvent::Exited {
            code: Some(1),
            stderr: String::new(),
        };
        record(failed, Some(SessionStatus::Errored));
        assert_eq!(snoozed(), None, "an error wakes it");
        let message = AgentEvent::UserMessage {
            text: "go on".to_string(),
            remote: true,
            at_reset: false,
            from_session: None,
        };
        record(message, Some(SessionStatus::Working));
        assert_eq!(snoozed(), None, "a message wakes it, from claude.ai too");
    }
}

//! What is worth telling the user about while they look elsewhere: a session that needs a
//! decision, has finished, has failed, or moved its task's worktree to another branch; and a
//! task's pull request whose checks failed or passed, that someone reviewed or commented on,
//! that conflicts with its base, or that was merged or closed.

use serde::Serialize;

use crate::agent::AgentEvent;
use crate::sessions::{SessionEvent, SessionStatus};
use crate::{Core, PullChange, Result};

/// The longest a notice's detail may be, in characters: it is read at a glance.
const MAX_DETAIL: usize = 140;

/// Something to tell the user about one session, for a front end to show as a notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notice {
    /// The session it is about.
    pub session_id: i64,
    /// What happened and to which session, such as "Needs you: Fix the login redirect".
    pub title: String,
    /// Where the session lives, and a line of detail if there is one.
    pub body: String,
}

/// What happened, and a line of detail. Nothing for events not worth an interruption.
///
/// A permission request only names the tool and the agent's own summary: the tool's arguments
/// may hold anything and are left out.
fn wording(event: &SessionEvent, spawned: bool) -> Option<(&'static str, String)> {
    match &event.event {
        AgentEvent::PermissionRequest {
            tool_name,
            description,
            ..
        } => {
            let detail = description
                .clone()
                .unwrap_or_else(|| format!("Wants to use {tool_name}"));
            Some(("Needs you", detail))
        }
        // A session another session started reports to that one, not to the user. A turn that
        // ends while sub-agents work on in the background hands nothing back yet: the core
        // marks it unread only once it does.
        AgentEvent::TurnDone {
            is_error: false,
            result,
            ..
        } if !spawned && event.unread == Some(true) => {
            Some(("Finished", result.clone().unwrap_or_default()))
        }
        AgentEvent::Exited { .. } if event.status == Some(SessionStatus::Errored) => {
            Some(("Stopped with an error", String::new()))
        }
        // Seen in the middle of a turn: the agent, or something it ran, moved the worktree,
        // and what the task shows and publishes follows it.
        AgentEvent::WorktreeMoved {
            to,
            between_turns: false,
            ..
        } => {
            let on = to.as_deref().unwrap_or("no branch");
            Some(("Moved its worktree", format!("Now on {on}")))
        }
        AgentEvent::PullChanged { change, .. } => Some(pull_wording(change)),
        _ => None,
    }
}

/// What changed on a pull request, and a line of detail: which checks, or who. Never what a
/// review or comment says, which anyone who can comment on the repository writes.
fn pull_wording(change: &PullChange) -> (&'static str, String) {
    let by = |who: &[String]| format!("By {}", who.join(", "));
    match change {
        PullChange::ChecksFailed { checks } => ("Checks failed", checks.join(", ")),
        PullChange::ChecksPassed => ("Checks passed", String::new()),
        PullChange::ChangesRequested { by: who } => ("Changes requested", by(who)),
        PullChange::Approved { by: who } => ("Approved", by(who)),
        PullChange::Reviewed { by: who } => ("Reviewed", by(who)),
        PullChange::Commented { by: who } => ("New comment", by(who)),
        PullChange::Conflicts => ("Conflicts with its base", String::new()),
        // Finished: the session it opens offers to put the task away.
        PullChange::Merged => ("Merged", "Put the task away?".to_string()),
        PullChange::Closed => ("Closed", String::new()),
    }
}

/// The first line of `text`, cut to what fits a notification.
fn glance(text: &str) -> String {
    let line = text.lines().find(|line| !line.trim().is_empty());
    let line = line.unwrap_or_default().trim();
    if line.chars().count() <= MAX_DETAIL {
        return line.to_string();
    }
    let cut: String = line.chars().take(MAX_DETAIL - 1).collect();
    format!("{cut}…")
}

impl Core {
    /// What to tell the user about `event`, if it is worth interrupting them and they have not
    /// turned notifications off. The front end decides whether they are looking already.
    pub fn notice_for(&self, event: &SessionEvent) -> Result<Option<Notice>> {
        // Asked for every event, pieces of a reply included: look nothing up for most of them.
        let candidate = matches!(
            event.event,
            AgentEvent::PermissionRequest { .. }
                | AgentEvent::TurnDone { .. }
                | AgentEvent::Exited { .. }
                | AgentEvent::WorktreeMoved { .. }
                | AgentEvent::PullChanged { .. }
        );
        if !candidate || !self.settings()?.notifications {
            return Ok(None);
        }
        // Stored by another Swarmed, which may show it itself.
        if event.elsewhere && !self.shows_notices_of(event.session_id) {
            return Ok(None);
        }
        let session = self.session(event.session_id)?;
        let Some((heading, detail)) = wording(event, session.spawned_by.is_some()) else {
            return Ok(None);
        };
        let task = self.store().task_by_id(session.task_id)?;
        let task_name = task.as_ref().map(|task| task.name.clone());
        let project = match &task {
            Some(task) => self.store().project_by_id(task.project_id)?,
            None => None,
        };
        let place = match (project, task) {
            // The chats' home has a name only Swarmed uses, which no window shows.
            (Some(project), Some(task)) if project.chats => task.name,
            (Some(project), Some(task)) => format!("{} / {}", project.name, task.name),
            _ => String::new(),
        };
        // A pull request is the task's, whichever session it is recorded in.
        let pull = match (&event.event, &task_name) {
            (AgentEvent::PullChanged { number, .. }, Some(task)) => {
                Some(format!("{task} #{number}"))
            }
            _ => None,
        };
        let name = match &pull {
            Some(pull) => pull.as_str(),
            None if session.title.is_empty() => "New session",
            None => &session.title,
        };
        let detail = glance(&detail);
        let body = [place, detail]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        Ok(Some(Notice {
            session_id: session.id,
            title: format!("{heading}: {name}"),
            body,
        }))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn event(status: Option<SessionStatus>, event: AgentEvent) -> SessionEvent {
        // An answer handed back makes the session unread, as the core decides.
        let unread = matches!(event, AgentEvent::TurnDone { .. }).then_some(true);
        SessionEvent {
            session_id: 1,
            status,
            unread,
            background: None,
            event,
            elsewhere: false,
        }
    }

    fn done(is_error: bool, result: Option<&str>) -> SessionEvent {
        let done = AgentEvent::TurnDone {
            is_error,
            result: result.map(str::to_string),
            cost_usd: None,
            duration_ms: None,
        };
        event(Some(SessionStatus::Idle), done)
    }

    #[test]
    fn a_permission_request_names_the_tool_and_never_its_arguments() {
        let asked = |description: Option<&str>| {
            let request = AgentEvent::PermissionRequest {
                request_id: "r1".to_string(),
                tool_name: "Bash".to_string(),
                input: json!({ "command": "curl -H 'Authorization: secret-token'" }), // gitleaks:allow
                description: description.map(str::to_string),
            };
            wording(&event(Some(SessionStatus::Waiting), request), false)
        };
        assert_eq!(
            asked(None),
            Some(("Needs you", "Wants to use Bash".to_string()))
        );
        assert_eq!(
            asked(Some("Push the branch")),
            Some(("Needs you", "Push the branch".to_string()))
        );
    }

    #[test]
    fn a_finished_turn_is_told_unless_another_session_started_it() {
        let finished = done(false, Some("All tests pass."));
        assert_eq!(
            wording(&finished, false),
            Some(("Finished", "All tests pass.".to_string()))
        );
        assert_eq!(wording(&finished, true), None);
        // A turn the user stopped is no news to them.
        assert_eq!(wording(&done(true, None), false), None);
    }

    #[test]
    fn an_agent_that_died_is_told_and_one_that_was_stopped_is_not() {
        let exited = || AgentEvent::Exited {
            code: Some(1),
            stderr: "boom".to_string(),
        };
        let failed = event(Some(SessionStatus::Errored), exited());
        assert_eq!(
            wording(&failed, true),
            Some(("Stopped with an error", String::new()))
        );
        assert_eq!(
            wording(&event(Some(SessionStatus::Idle), exited()), false),
            None
        );
        let text = AgentEvent::Text {
            text: "hello".to_string(),
        };
        assert_eq!(wording(&event(None, text), false), None);
    }

    #[test]
    fn a_pull_request_names_who_and_which_checks() {
        let changed = |change| {
            let event = AgentEvent::PullChanged {
                number: 142,
                url: "https://github.com/o/r/pull/142".to_string(),
                change,
            };
            wording(&event_of(event), false)
        };
        let checks = vec!["Test".to_string(), "Lint".to_string()];
        assert_eq!(
            changed(PullChange::ChecksFailed { checks }),
            Some(("Checks failed", "Test, Lint".to_string()))
        );
        let by = vec!["alice".to_string()];
        assert_eq!(
            changed(PullChange::Commented { by }),
            Some(("New comment", "By alice".to_string()))
        );
        assert_eq!(
            changed(PullChange::Merged),
            Some(("Merged", "Put the task away?".to_string()))
        );
    }

    fn event_of(event: AgentEvent) -> SessionEvent {
        self::event(None, event)
    }

    #[test]
    fn detail_is_one_line_that_fits() {
        assert_eq!(glance("\n  Done.  \nMore below."), "Done.");
        assert_eq!(glance(""), "");
        let long = "é".repeat(300);
        let cut = glance(&long);
        assert_eq!(cut.chars().count(), MAX_DETAIL);
        assert!(cut.ends_with('…'));
    }
}

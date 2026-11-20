//! What is worth telling the user about while they look elsewhere: a session that needs a
//! decision, has finished, or has failed.

use serde::Serialize;

use crate::agent::AgentEvent;
use crate::sessions::{SessionEvent, SessionStatus};
use crate::{Core, Result};

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
        // A session another session started reports to that one, not to the user.
        AgentEvent::TurnDone {
            is_error: false,
            result,
            ..
        } if !spawned => Some(("Finished", result.clone().unwrap_or_default())),
        AgentEvent::Exited { .. } if event.status == Some(SessionStatus::Errored) => {
            Some(("Stopped with an error", String::new()))
        }
        _ => None,
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
        );
        if !candidate || !self.settings()?.notifications {
            return Ok(None);
        }
        let session = self.session(event.session_id)?;
        let Some((heading, detail)) = wording(event, session.spawned_by.is_some()) else {
            return Ok(None);
        };
        let task = self.store().task_by_id(session.task_id)?;
        let project = match &task {
            Some(task) => self.store().project_by_id(task.project_id)?,
            None => None,
        };
        let place = match (project, task) {
            (Some(project), Some(task)) => format!("{} / {}", project.name, task.name),
            _ => String::new(),
        };
        let name = if session.title.is_empty() {
            "New session"
        } else {
            &session.title
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
        SessionEvent {
            session_id: 1,
            status,
            event,
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
                input: json!({ "command": "curl -H 'Authorization: secret-token'" }),
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
    fn detail_is_one_line_that_fits() {
        assert_eq!(glance("\n  Done.  \nMore below."), "Done.");
        assert_eq!(glance(""), "");
        let long = "é".repeat(300);
        let cut = glance(&long);
        assert_eq!(cut.chars().count(), MAX_DETAIL);
        assert!(cut.ends_with('…'));
    }
}

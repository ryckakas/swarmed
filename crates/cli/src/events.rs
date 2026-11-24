//! `swarmed events`: what happens in every session, as it happens, whoever does it.

use std::time::Duration;

use anyhow::Result;
use serde_json::Value;
use swarmed_core::agent::AgentEvent;
use swarmed_core::{Core, SessionEvent};
use tokio::sync::broadcast::error::RecvError;

use crate::say;
use crate::session::describe;

/// How often what other Swarmed processes wrote is looked for, as the app does.
const LOOK_EVERY: Duration = Duration::from_secs(1);

/// Prints every session's events until Ctrl-C: those of agents this command runs, which are
/// none, and those another Swarmed on the same data writes, noticed every second.
pub(crate) async fn run(core: &Core) -> Result<()> {
    let mut bus = core.subscribe();
    let mut looks = tokio::time::interval(LOOK_EVERY);
    loop {
        tokio::select! {
            published = bus.recv() => match published {
                Ok(published) => line_of(&published).into_iter().for_each(|line| say(&line)),
                Err(RecvError::Lagged(missed)) => say(&format!("({missed} events missed)")),
                Err(RecvError::Closed) => return Ok(()),
            },
            // One that failed is made again at the next look.
            _ = looks.tick() => core.notice_others().ok().unwrap_or_default(),
            _ = tokio::signal::ctrl_c() => return Ok(()),
        }
    }
}

/// The line an event is printed as: its session, or `-` for one that belongs to none, then
/// what happened, and the session's status if the event changed it. Nothing for the pieces of
/// a reply, whose whole follows.
fn line_of(published: &SessionEvent) -> Option<String> {
    let said = match &published.event {
        AgentEvent::TextDelta { .. } => return None,
        AgentEvent::Renamed { title } => format!("renamed: {title}"),
        AgentEvent::Marked { colour, label } => match colour {
            Some(colour) => format!("marked {}: {label}", colour.as_str()),
            None => "mark taken off".to_string(),
        },
        event => describe(event).unwrap_or_else(|| kind_of(event)),
    };
    let session = match published.session_id {
        0 => "-".to_string(),
        id => id.to_string(),
    };
    let status = published
        .status
        .map_or_else(String::new, |status| format!(" [{}]", status.as_str()));
    Some(format!("{session}\t{said}{status}"))
}

/// What kind of event `event` is, by the name it is stored under, such as `tree_changed`.
fn kind_of(event: &AgentEvent) -> String {
    let value = serde_json::to_value(event).unwrap_or_default();
    let kind = value.get("type").and_then(Value::as_str);
    kind.unwrap_or("event").replace('_', " ")
}

#[cfg(test)]
mod tests {
    use swarmed_core::SessionStatus;

    use super::*;

    fn published(session_id: i64, event: AgentEvent) -> SessionEvent {
        SessionEvent {
            session_id,
            status: None,
            unread: None,
            event,
            elsewhere: true,
        }
    }

    #[test]
    fn an_event_is_a_line_with_its_session_and_status() {
        let renamed = AgentEvent::Renamed {
            title: "Fix the login".to_string(),
        };
        assert_eq!(
            line_of(&published(4, renamed)).as_deref(),
            Some("4\trenamed: Fix the login")
        );
        let mut closed = published(4, AgentEvent::Closed);
        closed.status = Some(SessionStatus::Idle);
        assert_eq!(line_of(&closed).as_deref(), Some("4\tclosed [idle]"));
        let tree = published(0, AgentEvent::TreeChanged);
        assert_eq!(line_of(&tree).as_deref(), Some("-\ttree changed"));
        let piece = AgentEvent::TextDelta {
            text: "a".to_string(),
        };
        assert_eq!(line_of(&published(4, piece)), None);
    }
}

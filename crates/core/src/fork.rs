//! Forking a session: a new one that starts where an earlier conversation stood.
//!
//! No agent can be handed another's memory, so the fork is given the conversation as text. What
//! was said is copied into the new session, for the user to read, and goes to its agent with
//! the first message, whatever agent that is.

use crate::agent::AgentEvent;
use crate::sessions::{Session, SessionOptions};
use crate::{Core, Result, lock};

/// The most of an earlier conversation an agent is handed, in characters. Its end matters
/// most, so that is what is kept.
const MAX_CARRIED: usize = 60_000;

/// What of a conversation a fork starts with: what was said and shown, not how it came about.
fn kept(events: &[AgentEvent], upto: Option<usize>) -> Vec<AgentEvent> {
    let end = upto.map_or(events.len(), |upto| upto.min(events.len()));
    events[..end]
        .iter()
        .filter(|event| {
            matches!(
                event,
                AgentEvent::UserMessage { .. } | AgentEvent::Text { .. } | AgentEvent::View { .. }
            )
        })
        .cloned()
        .collect()
}

/// A conversation as text, each turn under who said it.
fn transcript(events: &[AgentEvent]) -> String {
    let turns: Vec<String> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::UserMessage { text } => Some(format!("User:\n{text}")),
            AgentEvent::Text { text } => Some(format!("Assistant:\n{text}")),
            _ => None,
        })
        .collect();
    let whole = turns.join("\n\n");
    let length = whole.chars().count();
    if length <= MAX_CARRIED {
        return whole;
    }
    let end: String = whole.chars().skip(length - MAX_CARRIED).collect();
    format!("[The beginning is left out.]\n…{end}")
}

/// What a fork's agent has to be told with the next message: the conversation it was forked
/// from, as long as nothing has been said in the fork itself yet.
pub(crate) fn carried(events: &[AgentEvent]) -> Option<String> {
    let forked = events
        .iter()
        .rposition(|event| matches!(event, AgentEvent::Forked { .. }))?;
    let said_since = events[forked..]
        .iter()
        .any(|event| matches!(event, AgentEvent::UserMessage { .. }));
    if said_since {
        return None;
    }
    let earlier = transcript(&events[..forked]);
    (!earlier.is_empty()).then_some(earlier)
}

/// `text` with the earlier conversation under it, the way the agent is to read it.
pub(crate) fn with_carried(text: &str, earlier: &str) -> String {
    format!(
        "{text}\n\n<earlier-conversation>\nThis session continues an earlier conversation, \
         which you do not otherwise remember. What was said in it:\n\n{earlier}\n\
         </earlier-conversation>"
    )
}

impl Core {
    /// A new session in the same task and on the same agent, starting with what was said in
    /// session `id`: all of it, or only its first `upto` events. Nothing runs until it is
    /// started, and its agent is given the conversation with the first message.
    pub fn session_fork(&self, id: i64, upto: Option<usize>) -> Result<Session> {
        let source = self.session(id)?;
        let earlier = kept(&self.session_events(id)?, upto);
        let title = if source.title.is_empty() {
            "Fork".to_string()
        } else {
            format!("{} (fork)", source.title)
        };
        let options = SessionOptions {
            agent: Some(source.agent),
            title: Some(title),
            model: source.model,
            supervision: source.supervision,
            effort: source.effort,
        };
        let store = lock(&self.store);
        let fork = store.session_insert(source.task_id, &options)?;
        store.session_copy_events(fork.id, &earlier, id)?;
        Ok(fork)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> AgentEvent {
        AgentEvent::UserMessage {
            text: text.to_string(),
        }
    }

    fn reply(text: &str) -> AgentEvent {
        AgentEvent::Text {
            text: text.to_string(),
        }
    }

    fn done() -> AgentEvent {
        AgentEvent::TurnDone {
            is_error: false,
            result: None,
            cost_usd: None,
        }
    }

    #[test]
    fn a_fork_keeps_what_was_said_up_to_the_point_asked_for() {
        let events = [
            user("one"),
            reply("two"),
            done(),
            user("three"),
            reply("four"),
        ];
        assert_eq!(
            kept(&events, None),
            [user("one"), reply("two"), user("three"), reply("four")]
        );
        assert_eq!(kept(&events, Some(3)), [user("one"), reply("two")]);
        assert_eq!(kept(&events, Some(99)).len(), 4);
        assert_eq!(kept(&events, Some(0)), []);
    }

    #[test]
    fn the_earlier_conversation_is_carried_until_something_is_said_in_the_fork() {
        let forked = AgentEvent::Forked { from: 7 };
        let fresh = [user("one"), reply("two"), forked.clone()];
        assert_eq!(
            carried(&fresh).as_deref(),
            Some("User:\none\n\nAssistant:\ntwo")
        );
        let continued = [user("one"), reply("two"), forked.clone(), user("three")];
        assert_eq!(carried(&continued), None);
        assert_eq!(carried(&[user("one")]), None);
        // A fork of nothing has nothing to hand on.
        assert_eq!(carried(&[forked]), None);
    }

    #[test]
    fn a_long_conversation_keeps_its_end() {
        let long = [user(&"a".repeat(MAX_CARRIED)), reply("the end")];
        let text = transcript(&long);
        assert!(text.starts_with("[The beginning is left out.]"));
        assert!(text.ends_with("the end"));
        assert!(text.chars().count() < MAX_CARRIED + 60);
    }

    #[test]
    fn the_agent_reads_the_message_first_and_the_history_marked_as_such() {
        let message = with_carried("Go on.", "User:\nhi");
        assert!(message.starts_with("Go on.\n\n<earlier-conversation>"));
        assert!(message.ends_with("User:\nhi\n</earlier-conversation>"));
    }
}

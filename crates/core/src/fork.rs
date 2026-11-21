//! Forking a session: a new one that starts where an earlier conversation stood.
//!
//! No agent can be handed another's memory, so the fork is given the conversation as text. What
//! was said is copied into the new session, for the user to read, and goes to its agent with
//! the first message, whatever agent that is.

use crate::agent::{AgentEvent, Supervision};
use crate::agents::CLAUDE;
use crate::sessions::{Session, SessionOptions};
use crate::{Core, Result, lock};

/// The most of an earlier conversation an agent is handed, in characters. Its end matters
/// most, so that is what is kept.
const MAX_CARRIED: usize = 60_000;

/// Where a fork that starts from message number `from` begins in a conversation, counting
/// from 0 every message sent into it: at that message, or at the very start if no message
/// is named or there is no such message.
///
/// A message is counted, not an event's place: a front end's list of events also holds ones
/// that are never stored, so a place in it says nothing of a place here.
fn start_of(events: &[AgentEvent], from: Option<usize>) -> usize {
    let messages = events
        .iter()
        .enumerate()
        .filter(|(_, event)| matches!(event, AgentEvent::UserMessage { .. }));
    let at = from.and_then(|from| messages.map(|(at, _)| at).nth(from));
    at.unwrap_or(0)
}

/// What of a conversation a fork starts with: what was said and shown from the message it is
/// forked from to the end, not how it came about, and nothing from before that message.
fn kept(events: &[AgentEvent], from: Option<usize>) -> Vec<AgentEvent> {
    let start = start_of(events, from);
    events[start..]
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

/// The supervision a clone on `agent` starts with: the source's, except that only Claude Code
/// plans before it acts, so a planning session asks instead on any other agent.
fn supervision_on(agent: &str, supervision: Supervision) -> Supervision {
    if supervision == Supervision::Plan && agent != CLAUDE {
        Supervision::Ask
    } else {
        supervision
    }
}

impl Core {
    /// A new session in the same task and on the same agent, starting with what was said in
    /// session `id`: all of it, or its message number `from` and everything after it,
    /// counting from 0 every message sent into it. What came before that message stays
    /// behind. The first session is not changed. Nothing runs until the new one is started,
    /// and its agent is given the conversation with the first message.
    ///
    /// With `agent`, the new session runs on that agent instead: a clone of the conversation
    /// for another agent to carry on. It gets what was said, as text, and not what the first
    /// agent did or read to say it. Model, effort and fast mode are that agent's own to start
    /// with, since they mean something else on every agent.
    pub fn session_fork(
        &self,
        id: i64,
        from: Option<usize>,
        agent: Option<&str>,
    ) -> Result<Session> {
        let source = self.session(id)?;
        let earlier = kept(&self.session_events(id)?, from);
        let title = if source.title.is_empty() {
            "Fork".to_string()
        } else {
            format!("{} (fork)", source.title)
        };
        let options = match agent.filter(|agent| *agent != source.agent) {
            None => SessionOptions {
                agent: Some(source.agent),
                title: Some(title),
                model: source.model,
                supervision: source.supervision,
                effort: source.effort,
                fast: source.fast,
            },
            Some(other) => {
                self.agents_now().backend(other)?;
                SessionOptions {
                    agent: Some(other.to_string()),
                    title: Some(title),
                    model: None,
                    supervision: supervision_on(other, source.supervision),
                    effort: None,
                    fast: false,
                }
            }
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
            duration_ms: None,
        }
    }

    #[test]
    fn a_clone_on_another_agent_asks_where_the_first_one_planned() {
        assert_eq!(supervision_on("codex", Supervision::Plan), Supervision::Ask);
        assert_eq!(supervision_on(CLAUDE, Supervision::Plan), Supervision::Plan);
        assert_eq!(
            supervision_on("codex", Supervision::Auto),
            Supervision::Auto
        );
    }

    #[test]
    fn a_fork_keeps_what_was_said_from_the_message_asked_for() {
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
        // From the second message: it and its answer, and nothing from before.
        assert_eq!(kept(&events, Some(1)), [user("three"), reply("four")]);
        assert_eq!(kept(&events, Some(0)).len(), 4);
        // No such message: all of it.
        assert_eq!(kept(&events, Some(99)).len(), 4);
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
